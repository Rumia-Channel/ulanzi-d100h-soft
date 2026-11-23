//! Config schema: rules, matches, actions, bindings.
//! Pure data — file I/O lives in the app crate so the core stays portable.
use serde::{Deserialize, Serialize};

use crate::InputId;

/// What an input does under a rule.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Do nothing (explicitly block lower-priority bindings for this input).
    None,
    /// Press a key chord, e.g. ["ctrl","alt","k"] or single ["j"].
    /// Key names are resolved by the platform layer (SendInput).
    Keys { keys: Vec<String> },
    /// Mouse-wheel notches with optional modifiers held, e.g. alt+wheel for
    /// timeline zoom. Positive = up (vertical) / right (horizontal).
    Wheel {
        #[serde(default = "default_notches")]
        notches: i32,
        #[serde(default)]
        horizontal: bool,
        #[serde(default)]
        modifiers: Vec<String>,
    },
    /// Named action for future built-in commands.
    Command { name: String },
}

/// When a rule applies.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Match {
    /// Foreground window belongs to this exe (e.g. "resolve.exe").
    Foreground { process: String },
    /// Process exists (any window), e.g. "notepad.exe".
    Running { process: String },
    /// Process does not exist.
    NotRunning { process: String },
    /// Always applies; the catch-all default profile.
    Always,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Rule {
    pub name: String,
    /// Higher wins. The first matching rule in descending priority is active.
    #[serde(default)]
    pub priority: i32,
    #[serde(rename = "match")]
    pub when: Match,
    /// input_id → action. Unlisted inputs are inert under this rule.
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Binding {
    pub input: InputId,
    pub action: Action,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

fn default_enabled() -> bool {
    true
}

fn default_notches() -> i32 {
    1
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: vec![Rule {
                name: "Default".to_string(),
                priority: 0,
                when: Match::Always,
                bindings: vec![],
            }],
        }
    }
}

impl Config {
    /// Parse config JSON; rules come out sorted by descending priority.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        let mut config: Config = serde_json::from_str(text)?;
        config.sort();
        Ok(config)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let mut config = self.clone();
        config.sort();
        serde_json::to_string_pretty(&config)
    }

    /// Reject bindings the engine could not execute (unknown key names,
    /// non-modifier wheel modifiers, zero notches).
    pub fn validate(&self) -> Result<(), String> {
        for rule in &self.rules {
            for binding in &rule.bindings {
                let problem = match &binding.action {
                    Action::Keys { keys } if keys.is_empty() => Some("empty key chord".to_string()),
                    Action::Keys { keys } => crate::keys::parse_chord(keys)
                        .err()
                        .map(|bad| format!("unknown key name(s): {}", bad.join(", "))),
                    Action::Wheel { notches: 0, .. } => Some("wheel notches must not be 0".to_string()),
                    Action::Wheel { modifiers, .. } => crate::keys::parse_modifiers(modifiers)
                        .err()
                        .map(|bad| format!("not a modifier key: {}", bad.join(", "))),
                    Action::None | Action::Command { .. } => None,
                };
                if let Some(problem) = problem {
                    return Err(format!("rule \"{}\", {}: {problem}", rule.name, binding.input.id()));
                }
            }
        }
        Ok(())
    }

    /// Canonical order: highest priority first.
    pub fn sort(&mut self) {
        self.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
    }
}
