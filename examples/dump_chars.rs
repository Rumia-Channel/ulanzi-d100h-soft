//! Dump every discovered service/characteristic of the connected Dial.
use std::{error::Error, time::Duration};

use btleplug::{
    api::{Central as _, Manager as _, Peripheral as _, RetrievePeripheralsOptions},
    platform::Manager,
};
use uuid::Uuid;

const SERVICE: Uuid = Uuid::from_u128(0x00001971_0000_1000_8000_00805f9b34fb);
const SERVICE_ALT: Uuid = Uuid::from_u128(0x00002013_0000_1000_8000_00805f9b34fb);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let manager = Manager::new().await?;
    for adapter in manager.adapters().await? {
        let peripherals = adapter
            .retrieve_peripherals(RetrievePeripheralsOptions {
                services: Some(vec![SERVICE, SERVICE_ALT]),
                ..Default::default()
            })
            .await?;
        println!("adapter found {} candidate(s)", peripherals.len());
        for peripheral in peripherals {
            println!("peripheral {}", peripheral.id());
            peripheral.connect().await?;
            peripheral.discover_services().await?;
            for service in peripheral.services() {
                println!("service {}", service.uuid);
                for c in &service.characteristics {
                    println!("  char {} props={:?}", c.uuid, c.properties);
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
            peripheral.disconnect().await?;
        }
    }
    Ok(())
}
