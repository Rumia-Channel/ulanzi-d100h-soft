//! Exercise write commands: read current haptics/sleep-timer, flip haptics,
//! change sleep timer, verify each response, then restore both.
use std::{error::Error, time::Duration};

use btleplug::{
    api::{Central as _, Manager as _, Peripheral as _, RetrievePeripheralsOptions, ScanFilter},
    platform::{Adapter, Manager, Peripheral},
};
use ulanzi_d100h::{ble::Client, Command, Message, Reply};
use uuid::Uuid;

const SERVICE: Uuid = Uuid::from_u128(0x00001971_0000_1000_8000_00805f9b34fb);
const SERVICE_ALT: Uuid = Uuid::from_u128(0x00002013_0000_1000_8000_00805f9b34fb);
const WRITE: Uuid = Uuid::from_u128(0x0000fd01_0000_1000_8000_00805f9b34fb);
const NOTIFY: Uuid = Uuid::from_u128(0x0000fd00_0000_1000_8000_00805f9b34fb);

async fn find_dial(manager: &Manager) -> Result<Option<(Adapter, Peripheral)>, Box<dyn Error>> {
    let services = vec![SERVICE, SERVICE_ALT];
    for adapter in manager.adapters().await? {
        for peripheral in adapter
            .retrieve_peripherals(RetrievePeripheralsOptions {
                services: Some(services.clone()),
                ..Default::default()
            })
            .await?
        {
            return Ok(Some((adapter.clone(), peripheral)));
        }
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

/// Send a command, then collect messages for `wait` ms.
async fn transact(client: &mut Client, command: Command, wait_ms: u64) -> Vec<Message> {
    if let Err(e) = client.send(command).await {
        eprintln!("send {command:?} failed: {e}");
    }
    tokio::time::sleep(Duration::from_millis(wait_ms)).await;
    let mut messages = Vec::new();
    while let Ok(Some(result)) =
        tokio::time::timeout(Duration::from_millis(150), client.next_message()).await
    {
        match result {
            Ok(message) => messages.push(message),
            Err(e) => eprintln!("notification error: {e}"),
        }
    }
    messages
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let manager = Manager::new().await?;
    let Some((_adapter, peripheral)) = find_dial(&manager).await? else {
        return Err("No Dial found.".into());
    };
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
        let mut client = Client::attach(peripheral.clone(), find(WRITE)?, find(NOTIFY)?).await?;
        client.send(Command::Heartbeat).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Baseline values.
        let haptics0 = transact(&mut client, Command::ReadHaptics, 300).await;
        let sleep0 = transact(&mut client, Command::ReadSleepTimer, 300).await;
        println!("baseline haptics  = {haptics0:?}");
        println!("baseline sleep    = {sleep0:?}");

        // 1) Toggle haptics, verify with read-back, restore.
        let toggled = transact(&mut client, Command::SetHaptics(false), 300).await;
        println!("SetHaptics(false) -> {toggled:?}");
        let verify = transact(&mut client, Command::ReadHaptics, 300).await;
        println!("read-back         -> {verify:?}");
        let restored = transact(&mut client, Command::SetHaptics(true), 300).await;
        println!("SetHaptics(true)  -> {restored:?}");

        // 2) Change sleep timer to a UI-tested value, verify, restore.
        let set = transact(&mut client, Command::SetSleepTimer(3600), 300).await;
        println!("SetSleepTimer(3600) -> {set:?}");
        let verify = transact(&mut client, Command::ReadSleepTimer, 300).await;
        println!("read-back           -> {verify:?}");
        let baseline_secs = sleep0.iter().find_map(|m| match m {
            Message::Reply(Reply::SleepTimer { seconds, .. }) => Some(*seconds),
            _ => None,
        });
        if let Some(secs) = baseline_secs {
            let restore = transact(&mut client, Command::SetSleepTimer(secs), 300).await;
            println!("SetSleepTimer({secs})  -> {restore:?}");
        }
        Ok(())
    }
    .await;
    if peripheral.is_connected().await? {
        peripheral.disconnect().await?;
    }
    result
}
