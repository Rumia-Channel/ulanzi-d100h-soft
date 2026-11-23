//! Speed Editor emulation lifecycle: keeps the usbip server running and
//! drives `usbip.exe attach` until the VHCI driver has imported the
//! virtual device. Detaches on shutdown.
//!
//! Requires usbip-win2 (usbip.exe + VHCI driver) on PATH or at
//! %ProgramFiles%\usbip\usbip.exe.

use std::time::Duration;
use dial_usbip::{Server, SpeedEditor};
use tokio::sync::watch;

const USBIP_PORT: u16 = 3240;

/// Spawn the emulation + attach supervision. Returns the shared device
/// handle for the engine to queue input into. Stops when `shutdown`
/// flips to true; best-effort detach of port 1 on the way out.
pub fn run(shutdown: watch::Receiver<bool>) -> SpeedEditor {
    let device = SpeedEditor::new();
    let server = Server::new(device.clone());
    tokio::spawn(async move {
        if let Err(e) = server.run(USBIP_PORT).await {
            eprintln!("usbip server failed: {e}");
        }
    });
    let d = device.clone();
    tokio::spawn(supervise(d, shutdown));
    device
}

async fn supervise(device: SpeedEditor, mut shutdown: watch::Receiver<bool>) {
    // usbip.exe path: prefer PATH, fall back to the default install dir.
    let exe = find_usbip();
    let Some(exe) = exe else {
        eprintln!("usbip: usbip.exe not found; Speed Editor emulation disabled");
        return;
    };
    loop {
        if *shutdown.borrow() {
            break;
        }
        if !device.attached() {
            match tokio::task::spawn_blocking({
                let exe = exe.clone();
                move || {
                    std::process::Command::new(exe)
                        .args(["attach", "-r", "127.0.0.1", "-b", "1-1"])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                }
            })
            .await
            {
                Ok(Ok(s)) if s.success() => eprintln!("usbip: attached"),
                Ok(Ok(s)) => eprintln!("usbip: attach exited {s}"),
                Ok(Err(e)) => eprintln!("usbip: attach failed: {e}"),
                Err(e) => eprintln!("usbip: attach task panicked: {e}"),
            }
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(Duration::from_secs(3)) => {}
        }
    }
    // Best-effort detach so the virtual device disappears cleanly.
    let _ = tokio::task::spawn_blocking(move || {
        std::process::Command::new(exe)
            .args(["detach", "-p", "1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    })
    .await;
}

fn find_usbip() -> Option<String> {
    for candidate in [
        std::env::var("USBIP_EXE").ok(),
        Some(r"C:\Program Files\usbip\usbip.exe".to_string()),
        Some(r"C:\Program Files\usbip-win2\usbip.exe".to_string()),
    ]
    .into_iter()
    .flatten()
    {
        if std::path::Path::new(&candidate).exists() {
            return Some(candidate);
        }
    }
    // PATH lookup as last resort.
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(';') {
            let p = std::path::Path::new(dir).join("usbip.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }
    }
    None
}
