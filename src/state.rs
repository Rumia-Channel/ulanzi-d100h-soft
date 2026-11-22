//! Shared state and serializable types for the web UI.
use std::collections::VecDeque;

use serde::Serialize;
use tokio::sync::{RwLock, broadcast, mpsc, watch};
use ulanzi_d100h::Command;

pub const FEED_CAPACITY: usize = 200;

/// Connection state shown in the UI and tray tooltip.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UiStatus {
    Scanning,
    Connected,
    Disconnected,
    Quitting,
}

/// Read-only device facts collected right after connect.
#[derive(Clone, Debug, Default, Serialize)]
pub struct DeviceInfo {
    pub firmware: Option<String>,
    pub serial: Option<String>,
    pub address: Option<String>,
    pub haptics_enabled: Option<bool>,
    pub sleep_timer_seconds: Option<u16>,
}

/// One line in the live feed.
#[derive(Clone, Debug, Serialize)]
pub struct FeedEntry {
    /// Milliseconds since UNIX epoch (UTC), for client-side rendering.
    pub at: u64,
    #[serde(flatten)]
    pub body: EntryBody,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub enum EntryBody {
    Event(EventInfo),
    Reply(ReplyInfo),
    Battery(BatteryInfo),
    Unknown { hex: String },
    Status { status: UiStatus, detail: Option<String> },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventInfo {
    Button { button: String, pressed: bool },
    KnobClick { pressed: bool },
    Rotation { direction: String, knob_pressed: bool },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum ReplyInfo {
    FirmwareVersion { version: String },
    DeviceAddress { address: String },
    Serial { serial: String },
    Haptics { enabled: bool, write_response: bool },
    SleepTimer { seconds: u16, write_response: bool },
}

#[derive(Clone, Debug, Serialize)]
pub struct BatteryInfo {
    pub percent: u8,
    pub voltage_raw: u16,
    pub charging: bool,
    pub charge_full: bool,
    pub power_off: bool,
}

/// What the UI may ask the BLE worker to do. Read-only by design.
#[derive(Debug)]
pub enum BleRequest {
    Query(Command),
    Reconnect,
}

/// Snapshot sent once to every new WebSocket client.
#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub status: UiStatus,
    pub device: DeviceInfo,
    pub feed: Vec<FeedEntry>,
}

/// Frame pushed to WebSocket clients.
#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsFrame {
    Snapshot { data: Snapshot },
    Entry { data: FeedEntry },
}

#[derive(Clone)]
pub struct AppState {
    pub status: watch::Sender<UiStatus>,
    pub device: std::sync::Arc<RwLock<DeviceInfo>>,
    pub feed_tx: broadcast::Sender<FeedEntry>,
    pub feed: std::sync::Arc<RwLock<VecDeque<FeedEntry>>>,
    pub ble_tx: mpsc::UnboundedSender<BleRequest>,
    /// Random token required on /api and /ws as ?token=…, printed as part of
    /// the tray-open URL. Protects the loopback server from other local pages.
    pub token: String,
    /// Broadcast to every task when the process should exit.
    pub shutdown: watch::Sender<bool>,
}

impl AppState {
    pub async fn push_entry(&self, body: EntryBody) {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let entry = FeedEntry { at, body };
        let mut feed = self.feed.write().await;
        feed.push_back(entry.clone());
        while feed.len() > FEED_CAPACITY {
            feed.pop_front();
        }
        drop(feed);
        // Lagged receivers are fine; the snapshot resync covers them.
        let _ = self.feed_tx.send(entry);
    }

    pub async fn snapshot(&self) -> Snapshot {
        let status = *self.status.borrow();
        Snapshot {
            status,
            device: self.device.read().await.clone(),
            feed: self.feed.read().await.iter().cloned().collect(),
        }
    }
}

/// Random per-boot token, hex-encoded. Uses getrandom (already pulled in by uuid).
pub fn generate_token() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // Extremely unlikely; fall back to time+pid entropy rather than a
        // hard-coded value.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            ^ u64::from(std::process::id()) as u128;
        bytes[..8].copy_from_slice(&(seed as u64).to_le_bytes());
        bytes[8..].copy_from_slice(&((seed >> 64) as u64).to_le_bytes());
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
