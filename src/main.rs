//! Connect to an Ulanzi D100H Dial and print decoded vendor events.
//! Adapted from the `events` example in https://github.com/kriomant/ulanzi-d100h.
use std::{error::Error, time::Duration};

use btleplug::{
    api::{Central as _, CentralEvent, Manager as _, Peripheral as _, RetrievePeripheralsOptions, ScanFilter},
    platform::{Adapter, Manager, Peripheral},
};
use futures_util::StreamExt as _;
use ulanzi_d100h::{ble::Client, Command, Message, Reply};
use uuid::Uuid;

// Confirmed vendor channel (firmware 1.3.8.1): service 1971, write fd01, notify fd00.
// Service 2013 is the alternate Studio service.
const SERVICE: Uuid = Uuid::from_u128(0x00001971_0000_1000_8000_00805f9b34fb);
const SERVICE_ALT: Uuid = Uuid::from_u128(0x00002013_0000_1000_8000_00805f9b34fb);
const WRITE: Uuid = Uuid::from_u128(0x0000fd01_0000_1000_8000_00805f9b34fb);
const NOTIFY: Uuid = Uuid::from_u128(0x0000fd00_0000_1000_8000_00805f9b34fb);

async fn find_dial(manager: &Manager) -> Result<Option<(Adapter, Peripheral)>, Box<dyn Error>> {
    let services = vec![SERVICE, SERVICE_ALT];
    for adapter in manager.adapters().await? {
        // Peripherals the OS already reports as connected (e.g. paired via Settings).
        for peripheral in adapter
            .retrieve_peripherals(RetrievePeripheralsOptions {
                services: Some(services.clone()),
                ..Default::default()
            })
            .await?
        {
            return Ok(Some((adapter.clone(), peripheral)));
        }
        // Otherwise scan briefly for the Dial's advertised name or service.
        adapter.start_scan(ScanFilter::default()).await?;
        tokio::time::sleep(Duration::from_secs(5)).await;
        adapter.stop_scan().await?;
        for peripheral in adapter.peripherals().await? {
            if let Some(props) = peripheral.properties().await? {
                if props.local_name.as_deref() == Some("Ulanzi Dial")
                    || props.services.iter().any(|s| services.contains(s))
                {
                    return Ok(Some((adapter.clone(), peripheral)));
                }
            }
        }
    }
    Ok(None)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let manager = Manager::new().await?;
    let Some((adapter, peripheral)) = find_dial(&manager).await? else {
        return Err("No Dial found. Pair/connect it in Bluetooth settings first.".into());
    };
    let id = peripheral.id();
    let mut central_events = adapter.events().await?;

    peripheral.connect().await?;
    let result: Result<(), Box<dyn Error>> = async {
        peripheral.discover_services().await?;
        let characteristics = peripheral.characteristics();
        let find = |uuid: Uuid| -> Result<_, Box<dyn Error>> {
            characteristics
                .iter()
                .find(|c| c.uuid == uuid)
                .cloned()
                .ok_or_else(|| format!("Characteristic {uuid} not found").into())
        };
        let write = find(WRITE)?;
        let notify = find(NOTIFY)?;
        println!("Connected to {id}: write={} notify={}", write.uuid, notify.uuid);

        let mut client = Client::attach(peripheral.clone(), write, notify).await?;
        // Heartbeats are required: without them the Dial returns to standalone
        // HID mode about three seconds after the last heartbeat.
        client.send(Command::Heartbeat).await?;
        let mut heartbeat = tokio::time::interval_at(
            tokio::time::Instant::now() + Duration::from_secs(1),
            Duration::from_secs(1),
        );
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        println!("Listening. Press Ctrl-C to stop.");
        loop {
            tokio::select! {
                _ = heartbeat.tick() => client.send(Command::Heartbeat).await?,
                message = client.next_message() => match message {
                    Some(Ok(Message::Reply(Reply::Heartbeat))) => {}
                    Some(Ok(message)) => println!("{message:?}"),
                    Some(Err(error)) => eprintln!("Notification error: {error}"),
                    None => break,
                },
                Some(event) = central_events.next() => {
                    if matches!(event, CentralEvent::DeviceDisconnected(device) if device == id) {
                        eprintln!("Device disconnected.");
                        break;
                    }
                }
            }
        }
        Ok(())
    }
    .await;

    if peripheral.is_connected().await? {
        peripheral.disconnect().await?;
    }
    result
}
