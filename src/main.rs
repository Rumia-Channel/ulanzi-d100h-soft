//! ulanzi-d100h-soft: background tray app that bridges the Ulanzi D100H dial
//! to a local read-only web UI. Design follows narou.rs's web server: tokio
//! runtime hosts axum + BLE worker, tray runs its own Win32 message loop on a
//! std::thread, loopback token guards /api and /ws.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ble;
mod state;
mod tray;
mod web;

use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast, mpsc, watch};

use state::{AppState, BleRequest, UiStatus, generate_token};

const DEFAULT_PORT: u16 = 17771;

#[tokio::main]
async fn main() {
    let port = std::env::var("ULANZI_D100H_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let token = generate_token();
    let url = format!("http://127.0.0.1:{port}/?token={token}");

    let (status_tx, _status_rx) = watch::channel(UiStatus::Scanning);
    let (feed_tx, _feed_rx) = broadcast::channel(256);
    let (ble_tx, ble_rx) = mpsc::unbounded_channel::<BleRequest>();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let app = AppState {
        status: status_tx,
        device: Arc::new(RwLock::new(state::DeviceInfo::default())),
        feed_tx,
        feed: Arc::new(RwLock::new(VecDeque::new())),
        ble_tx,
        token: token.clone(),
        shutdown: shutdown_tx,
    };

    // Windows tray icon on its own thread (Win32 message loop).
    #[cfg(windows)]
    tray::spawn(url.clone(), token.clone(), port);

    // axum on loopback.
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("cannot bind 127.0.0.1:{port}: {error}");
            return;
        }
    };
    let server = axum::serve(listener, web::router(app.clone()));

    // BLE worker.
    let ble_task = tokio::spawn(ble::run(app.clone(), ble_rx));

    println!("ulanzi-d100h-soft listening on {url}");

    let mut shutdown = shutdown_rx;
    tokio::select! {
        result = server => {
            if let Err(error) = result {
                eprintln!("server error: {error}");
            }
        }
        _ = async { while !*shutdown.borrow() { if shutdown.changed().await.is_err() { break } } } => {}
    }
    ble_task.abort();
}
