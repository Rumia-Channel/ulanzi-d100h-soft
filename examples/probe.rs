//! Query every read-only command and print the replies, then listen for events.
use std::{error::Error, time::Duration};

use btleplug::{
    api::{Central as _, Manager as _, Peripheral as _, RetrievePeripheralsOptions, ScanFilter},
    platform::{Adapter, Manager, Peripheral},
};
use ulanzi_d100h::{ble::Client, Command};
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

        // Send each read query, then drain messages for a moment so the matching
        // reply is printed next to its command.
        for command in [
            Command::ReadFirmwareVersion,
            Command::ReadSerial,
            Command::ReadDeviceAddress,
            Command::ReadFlashId,
            Command::ReadHaptics,
            Command::ReadSleepTimer,
        ] {
            println!(">>> {command:?}");
            client.send(command).await?;
            tokio::time::sleep(Duration::from_millis(300)).await;
            while let Ok(Some(message)) =
                tokio::time::timeout(Duration::from_millis(100), client.next_message()).await
            {
                println!("    {message:?}");
            }
        }

        println!("Listening for events/battery. Press Ctrl-C to stop.");
        loop {
            match tokio::time::timeout(Duration::from_secs(2), client.next_message()).await {
                Ok(Some(Ok(message))) => println!("{message:?}"),
                Ok(Some(Err(error))) => eprintln!("Notification error: {error}"),
                Ok(None) => break,
                Err(_) => client.send(Command::Heartbeat).await?,
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
