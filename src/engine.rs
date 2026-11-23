//! Action engine: watches the dial-event feed plus foreground/process sources,
//! resolves the active rule via dial-core, and executes bindings (key chords / mouse wheel via SendInput).
use std::{collections::HashSet, sync::Arc, time::Duration};

use dial_core::{Action, Config, InputId, Rule, World, active_rule};
use tokio::sync::{RwLock, broadcast, mpsc, watch};

use crate::state::{AppState, EntryBody, EventInfo, FeedEntry};
use crate::platform as process;
use dial_usbip::SpeedEditor;
/// Process table refresh cadence for running/not_running matches.
const PROC_REFRESH: Duration = Duration::from_secs(2);

pub struct Engine {
    state: AppState,
    config: Arc<RwLock<Config>>,
    /// Fires when the web layer saves a new config.
    reload_rx: watch::Receiver<()>,
    feed_rx: broadcast::Receiver<FeedEntry>,
    focus_rx: mpsc::UnboundedReceiver<String>,
    running: HashSet<String>,
    foreground: String,
    active: Option<usize>, // index into priority-sorted rules
    /// Emulated Speed Editor (usbip) for `se:` commands; `None` off Windows.
    speed_editor: Option<SpeedEditor>,
    /// Timestamp of the last rotation event, for velocity-sensitive jog.
    last_rotation: Option<std::time::Instant>,
}

impl Engine {
    pub fn new(
        state: AppState,
        reload_rx: watch::Receiver<()>,
        feed_rx: broadcast::Receiver<FeedEntry>,
        focus_rx: mpsc::UnboundedReceiver<String>,
        config: Arc<RwLock<Config>>,
        speed_editor: Option<SpeedEditor>,
    ) -> Self {
        Self {
            state,
            config,
            reload_rx,
            feed_rx,
            focus_rx,
            running: HashSet::new(),
            foreground: String::new(),
            active: None,
            speed_editor,
            last_rotation: None,
        }
    }

    pub async fn run(mut self) {
        self.running = process::running_processes();
        self.reconcile().await;
        let mut proc_tick = tokio::time::interval(PROC_REFRESH);
        proc_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut lagged = 0u64;

        loop {
            tokio::select! {
                entry = self.feed_rx.recv() => match entry {
                    Ok(entry) => {
                        self.on_entry(entry).await;
                        // Report only once a slot has been consumed: pushing
                        // into the still-full channel right after `Lagged`
                        // would evict our next unread entry and lag again.
                        if lagged > 0 {
                            let status = *self.state.status.borrow();
                            self.state
                                .push_entry(EntryBody::Status {
                                    status,
                                    detail: Some(format!("engine lagged: {lagged} feed entries dropped")),
                                })
                                .await;
                            lagged = 0;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => lagged += skipped,
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                name = self.focus_rx.recv() => match name {
                    Some(name) => {
                        if name != self.foreground {
                            self.foreground = name.clone();
                            *self.state.foreground.write().await = name.clone();
                            let status = *self.state.status.borrow();
                            self.state
                                .push_entry(EntryBody::Status {
                                    status,
                                    detail: Some(format!("foreground: {}", if name.is_empty() { "none" } else { &name })),
                                })
                                .await;
                            self.reconcile().await;
                        }
                    }
                    None => {} // hook thread gone; keep other sources alive
                },
                _ = proc_tick.tick() => {
                    let running = process::running_processes();
                    if running != self.running {
                        self.running = running;
                        self.reconcile().await;
                    }
                }
                changed = self.reload_rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    self.reconcile().await;
                }
            }
            if *self.state.shutdown.borrow() {
                return;
            }
        }
    }

    /// Re-evaluate the active rule and publish the result.
    async fn reconcile(&mut self) {
        let (name, detail, active) = {
            let config = self.config.read().await;
            let world = World { foreground: &self.foreground, running: &self.running };
            let active = active_rule(&config, &world);
            let name = active.map(|i| config.rules[i].name.clone());
            let detail = name.as_ref().map(|n| {
                format!(
                    "rule \"{n}\" active (foreground: {})",
                    if self.foreground.is_empty() { "none" } else { &self.foreground },
                )
            });
            (name, detail, active)
        };
        if active != self.active {
            self.active = active;
            *self.state.active_rule.write().await = name;
            if let Some(detail) = detail {
                let status = *self.state.status.borrow();
                self.state
                    .push_entry(EntryBody::Status { status, detail: Some(detail) })
                    .await;
            }
        }
    }

    async fn on_entry(&mut self, entry: FeedEntry) {
        let EntryBody::Event(event) = entry.body else {
            return;
        };
        let pressed = match &event {
            EventInfo::Button { pressed, .. } | EventInfo::KnobClick { pressed } => *pressed,
            EventInfo::Rotation { .. } => true,
        };
        if !pressed {
            return;
        }
        let Some(input) = input_id(&event) else {
            return;
        };
        // Velocity estimate for the jog: counts per tick scale with how
        // fast ticks arrive, like the Speed Editor's own encoder.
        let mut jog_scale = 1i32;
        if let EventInfo::Rotation { .. } = event {
            let now = std::time::Instant::now();
            if let Some(prev) = self.last_rotation {
                let dt_ms = now.duration_since(prev).as_millis() as f32;
                // ~17 counts/tick at 14ms, 1 count at ~4s+ pauses.
                jog_scale = ((0.25 / dt_ms.max(0.001) * 1000.0).round() as i32).clamp(1, 256);
            }
            self.last_rotation = Some(now);
        }
        // Resolve the action under the lock, drop it, then act — keeps the
        // future Send and the lock hold short.
        let action = {
            let config = self.config.read().await;
            self.active
                .and_then(|i| config.rules.get(i))
                .and_then(|rule| binding_action(rule, input))
        };
        let Some(action) = action else {
            return;
        };
        let result = match action {
            Action::None => Ok(()),
            Action::Keys { keys } => process::send_keys(&keys).map_err(|e| e.to_string()),
            Action::Wheel { notches, horizontal, modifiers } => {
                process::send_wheel(&modifiers, notches, horizontal).map_err(|e| e.to_string())
            }
            Action::Command { name } => self.run_command(&name, input, jog_scale).await,
        };
        if let Err(detail) = result {
            let status = *self.state.status.borrow();
            self.state
                .push_entry(EntryBody::Status { status, detail: Some(format!("{}: {detail}", input.id())) })
                .await;
        }
    }

    /// Named command actions dispatched through the emulated Speed Editor.
    /// "se:jog" feeds rotation into the wheel; "se:key:<name>" taps a
    /// transport key. Anything else returns an error so it surfaces in
    /// the feed.
    async fn run_command(&self, name: &str, input: InputId, jog_scale: i32) -> Result<(), String> {
        let Some(se) = &self.speed_editor else {
            return Err(format!("command action \"{name}\" unavailable (no Speed Editor)"));
        };
        match name {
            "se:jog" => {
                let ticks = match input {
                    InputId::RotateCw | InputId::RotateHeldCw => 1,
                    InputId::RotateCcw | InputId::RotateHeldCcw => -1,
                    _ => 0,
                };
                if ticks == 0 {
                    return Err(format!("se:jog bound to non-rotation input {}", input.id()));
                }
                if !se.authenticated() {
                    return Err("se:jog: Speed Editor not authenticated".into());
                }
                se.queue_jog(ticks, jog_scale);
                Ok(())
            }
            _ if name.starts_with("se:key:") => {
                let key_name = &name[7..];
                let Some(code) = dial_usbip::key_by_name(key_name) else {
                    return Err(format!("se:key: unknown key \"{key_name}\""));
                };
                if !se.authenticated() {
                    return Err("se:key: Speed Editor not authenticated".into());
                }
                se.tap_key(code);
                Ok(())
            }
            _ => Err(format!("command action \"{name}\" not implemented")),
        }
    }
}

/// Action bound to `input`. Knob-held rotation without its own binding falls
/// back to the plain rotation binding, so turning while pressed still works.
fn binding_action(rule: &Rule, input: InputId) -> Option<Action> {
    let find = |id: InputId| rule.bindings.iter().find(|b| b.input == id).map(|b| b.action.clone());
    find(input).or_else(|| match input {
        InputId::RotateHeldCw => find(InputId::RotateCw),
        InputId::RotateHeldCcw => find(InputId::RotateCcw),
        _ => None,
    })
}

fn input_id(event: &EventInfo) -> Option<InputId> {
    Some(match event {
        EventInfo::Button { button, .. } => match button.as_str() {
            "TopLeft" => InputId::ButtonTopLeft,
            "TopMiddle" => InputId::ButtonTopMiddle,
            "TopRight" => InputId::ButtonTopRight,
            "UpperLeft" => InputId::ButtonUpperLeft,
            "UpperRight" => InputId::ButtonUpperRight,
            "LowerLeft" => InputId::ButtonLowerLeft,
            "LowerRight" => InputId::ButtonLowerRight,
            _ => return None,
        },
        EventInfo::KnobClick { .. } => InputId::KnobClick,
        EventInfo::Rotation { direction, knob_pressed } => {
            match (direction.as_str(), knob_pressed) {
                ("Clockwise", false) => InputId::RotateCw,
                ("Counterclockwise", false) => InputId::RotateCcw,
                ("Clockwise", true) => InputId::RotateHeldCw,
                ("Counterclockwise", true) => InputId::RotateHeldCcw,
                _ => return None,
            }
        }
    })
}
