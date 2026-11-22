//! ulanzi-d100h-soft: background tray app that bridges the Ulanzi D100H dial
//! to a local web UI and drives per-app key mappings. Design follows narou.rs:
//! tokio hosts axum + BLE worker + rule engine; tray runs its own Win32 loop.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ble;
mod config;
mod engine;
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
    let (reload_tx, reload_rx) = watch::channel(());
    let config = Arc::new(RwLock::new(config::load()));

    let app = AppState {
        status: status_tx,
        device: Arc::new(RwLock::new(state::DeviceInfo::default())),
        feed_tx,
        feed: Arc::new(RwLock::new(VecDeque::new())),
        active_rule: Arc::new(RwLock::new(None)),
        ble_tx,
        token: token.clone(),
        shutdown: shutdown_tx,
        config: config.clone(),
        foreground: Arc::new(RwLock::new(String::new())),
        config_reload: reload_tx,
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

    // BLE worker + rule engine + foreground watcher.
    let ble_task = tokio::spawn(ble::run(app.clone(), ble_rx));
    let focus_rx = dial_win32::focus_events();
    let engine = engine::Engine::new(
        app.clone(),
        reload_rx,
        app.feed_tx.subscribe(),
        focus_rx,
        config,
    );
    let engine_task = tokio::spawn(engine.run());

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
    engine_task.abort();
}
