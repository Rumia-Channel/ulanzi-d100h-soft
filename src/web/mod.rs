//! Local web UI: axum server on loopback, JSON API + WebSocket feed.
//! Mirrors narou.rs's design at read-only scale: query-token auth on /api and
//! /ws, embedded assets, {"type": ...} JSON frames.
pub mod assets;

use axum::{
    Json, Router,
    extract::{Path, Query, State, WebSocketUpgrade, ws::Message as WsMessage},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post, put},
};
use futures_util::StreamExt as _;
use serde::Deserialize;
use tokio::sync::broadcast;
use ulanzi_d100h::Command;

use crate::state::{AppState, BleRequest, Snapshot, UiStatus, WsFrame};

#[derive(Deserialize)]
pub struct TokenQuery {
    token: Option<String>,
}

#[derive(Deserialize)]
pub struct CommandQuery {
    token: Option<String>,
    name: Option<String>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { assets::index() }))
        .route("/assets/{*path}", get(|Path(path): Path<String>| async move {
            assets::asset(&path)
        }))
        .route("/api/status", get(api_status))
        .route("/api/query", get(api_query))
        .route("/api/query", post(api_query))
        .route("/api/reconnect", post(api_reconnect))
        .route("/api/config", get(api_config_get))
        .route("/api/config", put(api_config_put))
        .route("/api/shutdown", post(api_shutdown))
        .route("/api/command", post(api_command))
        .route("/api/processes", get(api_processes))
        .route("/api/foreground", get(api_foreground))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

fn authorized(state: &AppState, token: &Option<String>) -> bool {
    token.as_deref() == Some(state.token.as_str())
}
async fn api_status(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
) -> Result<Json<Snapshot>, StatusCode> {
    if !authorized(&state, &q.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(state.snapshot().await))
}

fn command_by_name(name: &str) -> Option<Command> {
    Some(match name {
        "heartbeat" => Command::Heartbeat,
        "firmware" => Command::ReadFirmwareVersion,
        "serial" => Command::ReadSerial,
        "address" => Command::ReadDeviceAddress,
        "flash_id" => Command::ReadFlashId,
        "haptics" => Command::ReadHaptics,
        "sleep_timer" => Command::ReadSleepTimer,
        _ => return None,
    })
}

async fn api_query(
    State(state): State<AppState>,
    Query(q): Query<CommandQuery>,
) -> StatusCode {
    if !authorized(&state, &q.token) {
        return StatusCode::UNAUTHORIZED;
    }
    match q.name.as_deref().and_then(command_by_name) {
        Some(command) => {
            let _ = state.ble_tx.send(BleRequest::Query(command));
            StatusCode::NO_CONTENT
        }
        None => StatusCode::BAD_REQUEST,
    }
}

async fn api_reconnect(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
) -> StatusCode {
    if !authorized(&state, &q.token) {
        return StatusCode::UNAUTHORIZED;
    }
    let _ = state.ble_tx.send(BleRequest::Reconnect);
    StatusCode::NO_CONTENT
}

/// GET /api/config?token=… — current mapping rules as JSON.
async fn api_config_get(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
) -> Result<Json<dial_core::Config>, StatusCode> {
    if !authorized(&state, &q.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(state.config.read().await.clone()))
}

/// PUT /api/config?token=… — validate, save to disk, hot-reload the engine.
async fn api_config_put(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
    body: String,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !authorized(&state, &q.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let config = match dial_core::Config::from_json(&body) {
        Ok(config) => config,
        Err(error) => {
            return Ok(Json(serde_json::json!({
                "saved": false,
                "error": error.to_string(),
            })));
        }
    };
    *state.config.write().await = config.clone();
    let saved = match crate::config::save(&config) {
        Ok(()) => {
            let _ = state.config_reload.send(());
            true
        }
        Err(_) => false,
    };
    Ok(Json(serde_json::json!({ "saved": saved })))
}

#[derive(Deserialize)]
pub struct CommandBody {
    name: String,
    /// For set commands: bool for haptics, seconds for sleep timer.
    value: Option<serde_json::Value>,
}

/// POST /api/command?token=… — includes write commands now that the UI is the
/// control surface. Body: {"name": "…", "value": …}.
async fn api_command(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
    Json(body): Json<CommandBody>,
) -> StatusCode {
    if !authorized(&state, &q.token) {
        return StatusCode::UNAUTHORIZED;
    }
    let command = match body.name.as_str() {
        "set_haptics" => match body.value.as_ref().and_then(|v| v.as_bool()) {
            Some(enabled) => Command::SetHaptics(enabled),
            None => return StatusCode::BAD_REQUEST,
        },
        "set_sleep_timer" => match body.value.as_ref().and_then(|v| v.as_u64()) {
            Some(secs) if secs <= u64::from(u16::MAX) => Command::SetSleepTimer(secs as u16),
            _ => return StatusCode::BAD_REQUEST,
        },
        name => match command_by_name(name) {
            Some(command) => command,
            None => return StatusCode::BAD_REQUEST,
        },
    };
    let _ = state.ble_tx.send(BleRequest::Query(command));
    StatusCode::NO_CONTENT
}

/// GET /api/processes?token=… — sorted list of running exe names for the
/// rule editor's autocomplete.
async fn api_processes(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
) -> Result<Json<Vec<String>>, StatusCode> {
    if !authorized(&state, &q.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut names: Vec<String> = tokio::task::spawn_blocking(dial_win32::running_processes)
        .await
        .map(|set| set.into_iter().collect())
        .unwrap_or_default();
    names.sort();
    Ok(Json(names))
}

/// GET /api/foreground?token=… — exe owning the current foreground window and
/// the rule currently in effect.
async fn api_foreground(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !authorized(&state, &q.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(serde_json::json!({
        "foreground": *state.foreground.read().await,
        "active_rule": *state.active_rule.read().await,
    })))
}

/// Tray menu exit lands here (narou.rs pattern): reply, then exit the process
/// so the tray thread and every task die together.
async fn api_shutdown(State(state): State<AppState>, headers: HeaderMap) -> StatusCode {
    let ok = headers
        .get("x-internal-token")
        .and_then(|v| v.to_str().ok())
        == Some(state.token.as_str());
    if !ok {
        return StatusCode::UNAUTHORIZED;
    }
    let _ = state.status.send(UiStatus::Quitting);
    let _ = state.shutdown.send(true);
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::exit(0);
    });
    StatusCode::NO_CONTENT
}

async fn ws_handler(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, StatusCode> {
    if !authorized(&state, &q.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(ws.on_upgrade(move |socket| handle_socket(state, socket)))
}

async fn handle_socket(state: AppState, mut socket: axum::extract::ws::WebSocket) {
    let mut feed_rx = state.feed_tx.subscribe();

    // Initial snapshot, then live entries.
    let snapshot = state.snapshot().await;
    if send_json(&mut socket, &WsFrame::Snapshot { data: snapshot }).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            entry = feed_rx.recv() => match entry {
                Ok(entry) => {
                    if send_json(&mut socket, &WsFrame::Entry { data: entry }).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Missed entries: resync with a fresh snapshot.
                    let snapshot = state.snapshot().await;
                    if send_json(&mut socket, &WsFrame::Snapshot { data: snapshot }).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            incoming = socket.next() => match incoming {
                Some(Ok(WsMessage::Text(text))) => {
                    handle_client_message(&state, &text).await;
                }
                Some(Ok(_)) => {} // pings/binary ignored
                Some(Err(_)) | None => return,
            },
        }
    }
}

/// Client → server frames: {"type":"query","name":"firmware"} or
/// {"type":"reconnect"}.
async fn handle_client_message(state: &AppState, text: &str) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    match value.get("type").and_then(|t| t.as_str()) {
        Some("query") => {
            if let Some(command) = value
                .get("name")
                .and_then(|n| n.as_str())
                .and_then(command_by_name)
            {
                let _ = state.ble_tx.send(BleRequest::Query(command));
            }
        }
        Some("reconnect") => {
            let _ = state.ble_tx.send(BleRequest::Reconnect);
        }
        _ => {}
    }
}

async fn send_json(
    socket: &mut axum::extract::ws::WebSocket,
    frame: &WsFrame,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(frame).map_err(|e| axum::Error::new(e))?;
    socket.send(WsMessage::Text(text.into())).await
}
