//! BLE worker: keeps a session with the Dial alive and forwards everything to
//! the shared state. Read-only: only Query commands are ever sent.
use std::{collections::HashMap, error::Error, time::Duration};

use btleplug::{
    api::{Central as _, CentralEvent, Manager as _, Peripheral as _, RetrievePeripheralsOptions, ScanFilter},
    platform::{Adapter, Manager, Peripheral},
};
use futures_util::StreamExt as _;
use tokio::sync::{mpsc, watch};
use ulanzi_d100h::{ble::Client, BatteryStatus, Command, Event, Message, Reply};
use uuid::Uuid;

use crate::state::{AppState, BatteryInfo, EntryBody, EventInfo, ReplyInfo, UiStatus};

// Confirmed vendor channel (firmware 1.3.8.1): service 1971, write fd01, notify fd00.
const SERVICE: Uuid = Uuid::from_u128(0x00001971_0000_1000_8000_00805f9b34fb);
const SERVICE_ALT: Uuid = Uuid::from_u128(0x00002013_0000_1000_8000_00805f9b34fb);
const WRITE: Uuid = Uuid::from_u128(0x0000fd01_0000_1000_8000_00805f9b34fb);
const NOTIFY: Uuid = Uuid::from_u128(0x0000fd00_0000_1000_8000_00805f9b34fb);

const RECONNECT_MAX_SECS: u64 = 30;

/// Main worker loop: (re)connect, serve until the link drops, back off, retry.
pub async fn run(state: AppState, mut requests: mpsc::UnboundedReceiver<BleRequestBox>) {
    let _ = state.status.send(UiStatus::Scanning);
    let manager = match Manager::new().await {
        Ok(manager) => manager,
        Err(error) => {
            state
                .push_entry(EntryBody::Status {
                    status: UiStatus::Disconnected,
                    detail: Some(format!("BLE adapter unavailable: {error}")),
                })
                .await;
            return;
        }
    };

    let mut backoff = Duration::from_secs(2);
    loop {
        if *state.shutdown.borrow() {
            return;
        }
        set_status(&state, UiStatus::Scanning).await;
        match session(&manager, &state, &mut requests).await {
            Ok(()) => return, // clean shutdown
            Err(error) => {
                set_status(&state, UiStatus::Disconnected).await;
                state
                    .push_entry(EntryBody::Status {
                        status: UiStatus::Disconnected,
                        detail: Some(error),
                    })
                    .await;
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {},
                    _ = wait_shutdown(state.shutdown.subscribe()) => return,
                }
                backoff = (backoff * 2).min(Duration::from_secs(RECONNECT_MAX_SECS));
            }
        }
    }
}

/// Wraps requests so the signature above stays readable.
pub type BleRequestBox = crate::state::BleRequest;

async fn wait_shutdown(mut rx: watch::Receiver<bool>) {
    while !*rx.borrow() {
        if rx.changed().await.is_err() {
            return;
        }
    }
}

async fn set_status(state: &AppState, status: UiStatus) {
    let _ = state.status.send(status);
    state.push_entry(EntryBody::Status { status, detail: None }).await;
}

/// One connection lifetime. Returns Ok only when shutting down.
async fn session(
    manager: &Manager,
    state: &AppState,
    requests: &mut mpsc::UnboundedReceiver<BleRequestBox>,
) -> Result<(), String> {
    let (adapter, peripheral) = find_dial(manager, state).await.map_err(|e| e.to_string())?;
    let id = peripheral.id();
    let mut central_events = adapter.events().await.map_err(|e| e.to_string())?;

    peripheral.connect().await.map_err(|e| e.to_string())?;
    peripheral.discover_services().await.map_err(|e| e.to_string())?;

    // The vendor characteristics can take a moment to appear after service
    // discovery on Windows — poll briefly before giving up.
    let mut write = None;
    let mut notify = None;
    for _ in 0..10 {
        let characteristics = peripheral.characteristics();
        write = characteristics.iter().find(|c| c.uuid == WRITE).cloned();
        notify = characteristics.iter().find(|c| c.uuid == NOTIFY).cloned();
        if write.is_some() && notify.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let write = write.ok_or_else(|| format!("characteristic {WRITE} not found"))?;
    let notify = notify.ok_or_else(|| format!("characteristic {NOTIFY} not found"))?;

    let mut client = Client::attach(peripheral.clone(), write, notify).await.map_err(|e| e.to_string())?;
    client.send(Command::Heartbeat).await.map_err(|e| e.to_string())?;
    set_status(state, UiStatus::Connected).await;
    state
        .push_entry(EntryBody::Status {
            status: UiStatus::Connected,
            detail: Some(format!("connected to {id}")),
        })
        .await;

    // Populate device info right away; replies arrive interleaved with events.
    // Writes are without-response, so space them out — the device drops
    // commands sent in a tight burst (observed: middle commands lost).
    for command in [
        Command::ReadFirmwareVersion,
        Command::ReadSerial,
        Command::ReadDeviceAddress,
        Command::ReadHaptics,
        Command::ReadSleepTimer,
    ] {
        client.send(command).await.map_err(|e| e.to_string())?;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut serial_parts: HashMap<u8, Vec<u8>> = HashMap::new();
    let mut shutdown_rx = state.shutdown.subscribe();

    let result: Result<(), String> = loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if let Err(e) = client.send(Command::Heartbeat).await {
                    break Err(e.to_string());
                }
            }
            request = requests.recv() => match request {
                Some(BleRequestBox::Query(command)) => {
                    if let Err(e) = client.send(command).await {
                        break Err(e.to_string());
                    }
                }
                Some(BleRequestBox::Reconnect) => break Err("reconnect requested".into()),
                None => {
                    // UI channel closed; keep serving until shutdown.
                    futures_util::future::pending::<()>().await;
                }
            },
            message = client.next_message() => match message {
                Some(Ok(message)) => {
                    if let Some(body) = decode(message, &mut serial_parts, state).await {
                        state.push_entry(body).await;
                    }
                }
                Some(Err(error)) => {
                    state
                        .push_entry(EntryBody::Status {
                            status: UiStatus::Connected,
                            detail: Some(format!("notification error: {error}")),
                        })
                        .await;
                }
                None => break Err("notification stream ended".into()),
            },
            Some(event) = central_events.next() => {
                if matches!(event, CentralEvent::DeviceDisconnected(device) if device == id) {
                    break Err("device disconnected".into());
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    set_status(state, UiStatus::Quitting).await;
                    client.disconnect().await.ok();
                    break Ok(());
                }
            }
        }
    };

    if peripheral.is_connected().await.unwrap_or(false) {
        peripheral.disconnect().await.ok();
    }
    result
}

/// Convert a protocol message into a feed entry, updating device info and the
/// serial reassembly buffer as a side effect.
async fn decode(
    message: Message,
    serial_parts: &mut HashMap<u8, Vec<u8>>,
    state: &AppState,
) -> Option<EntryBody> {
    match message {
        Message::Event(Event::Button { button, pressed }) => Some(EntryBody::Event(EventInfo::Button {
            button: format!("{button:?}"),
            pressed,
        })),
        Message::Event(Event::KnobClick { pressed }) => {
            Some(EntryBody::Event(EventInfo::KnobClick { pressed }))
        }
        Message::Event(Event::Rotation { direction, knob_pressed }) => {
            Some(EntryBody::Event(EventInfo::Rotation {
                direction: format!("{direction:?}"),
                knob_pressed,
            }))
        }
        Message::BatteryStatus(BatteryStatus {
            voltage_raw,
            battery_percent,
            charging,
            charge_full,
            power_off,
            ..
        }) => Some(EntryBody::Battery(BatteryInfo {
            percent: battery_percent,
            voltage_raw,
            charging,
            charge_full,
            power_off,
        })),
        Message::Reply(Reply::Heartbeat) => None, // too noisy for the feed
        Message::Reply(Reply::FirmwareVersion { version, .. }) => {
            let text = version.to_string();
            state.device.write().await.firmware = Some(text.clone());
            Some(EntryBody::Reply(ReplyInfo::FirmwareVersion { version: text }))
        }
        Message::Reply(Reply::DeviceAddress(bytes)) => {
            let address = bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
            state.device.write().await.address = Some(address.clone());
            Some(EntryBody::Reply(ReplyInfo::DeviceAddress { address }))
        }
        Message::Reply(Reply::SerialFragment { index, text }) => {
            serial_parts.insert(index, text);
            if serial_parts.len() >= 2 && serial_parts.contains_key(&0) && serial_parts.contains_key(&1) {
                let mut bytes = Vec::new();
                for index in 0..=1u8 {
                    bytes.extend_from_slice(&serial_parts[&index]);
                }
                let serial = String::from_utf8_lossy(&bytes).to_string();
                state.device.write().await.serial = Some(serial.clone());
                serial_parts.clear();
                Some(EntryBody::Reply(ReplyInfo::Serial { serial }))
            } else {
                None // wait for the other fragment
            }
        }
        Message::Reply(Reply::Haptics { enabled, write_response }) => {
            state.device.write().await.haptics_enabled = Some(enabled);
            Some(EntryBody::Reply(ReplyInfo::Haptics { enabled, write_response }))
        }
        Message::Reply(Reply::SleepTimer { seconds, write_response }) => {
            state.device.write().await.sleep_timer_seconds = Some(seconds);
            Some(EntryBody::Reply(ReplyInfo::SleepTimer { seconds, write_response }))
        }
        // FlashId etc.: show raw to keep the feed honest without inventing meaning.
        Message::Reply(other) => Some(EntryBody::Unknown {
            hex: format!("{other:?}"),
        }),
        Message::Unknown(packet) => Some(EntryBody::Unknown {
            hex: packet.0.iter().map(|b| format!("{b:02x}")).collect(),
        }),
    }
}

async fn find_dial(
    manager: &Manager,
    state: &AppState,
) -> Result<(Adapter, Peripheral), Box<dyn Error>> {
    let services = vec![SERVICE, SERVICE_ALT];
    for adapter in manager.adapters().await? {
        for peripheral in adapter
            .retrieve_peripherals(RetrievePeripheralsOptions {
                services: Some(services.clone()),
                ..Default::default()
            })
            .await?
        {
            return Ok((adapter.clone(), peripheral));
        }
        adapter.start_scan(ScanFilter::default()).await?;
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(5)) => {},
            _ = wait_shutdown(state.shutdown.subscribe()) => {
                return Err("shutdown".into());
            }
        }
        adapter.stop_scan().await?;
        for peripheral in adapter.peripherals().await? {
            if let Some(props) = peripheral.properties().await? {
                if props.local_name.as_deref() == Some("Ulanzi Dial")
                    || props.services.iter().any(|s| services.contains(s))
                {
                    return Ok((adapter.clone(), peripheral));
                }
            }
        }
    }
    Err("no Dial found".into())
}
