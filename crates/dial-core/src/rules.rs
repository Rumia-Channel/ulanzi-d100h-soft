//! Pure rule matching: given the world state, which rule is active?
use std::collections::HashSet;

use crate::{Config, Match};

/// Snapshot of the inputs rule matching cares about.
pub struct World<'a> {
    /// Lowercase exe name owning the foreground window; empty when unknown.
    pub foreground: &'a str,
    /// Lowercase exe names of all running processes.
    pub running: &'a HashSet<String>,
}

/// Index into `config.rules` of the highest-priority matching rule.
/// Does not require the rules to be sorted.
pub fn active_rule(config: &Config, world: &World) -> Option<usize> {
    if !config.enabled {
        return None;
    }
    config
        .rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| matches(&rule.when, world))
        .max_by_key(|(_, rule)| rule.priority)
        .map(|(i, _)| i)
}

/// Lowercased process name without a trailing ".exe", so one profile written
/// as "resolve.exe" also matches Linux's "resolve" (and vice versa).
fn process_key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    match lower.strip_suffix(".exe") {
        Some(stem) => stem.to_string(),
        None => lower,
    }
}

fn is_running(process: &str, world: &World) -> bool {
    let key = process_key(process);
    world.running.iter().any(|name| process_key(name) == key)
}

fn matches(when: &Match, world: &World) -> bool {
    match when {
        Match::Foreground { process } => process_key(world.foreground) == process_key(process),
        Match::Running { process } => is_running(process, world),
        Match::NotRunning { process } => !is_running(process, world),
        Match::Always => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world<'a>(fg: &'a str, running: &'a HashSet<String>) -> World<'a> {
        World { foreground: fg, running }
    }

    fn procs(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_lowercase()).collect()
    }

    fn rule(name: &str, priority: i32, when: Match) -> crate::Rule {
        crate::Rule { name: name.into(), priority, when, bindings: vec![] }
    }

    #[test]
    fn highest_priority_wins() {
        let running = procs(&["resolve.exe", "chrome.exe"]);
        let config = Config {
            enabled: true,
            rules: vec![
                rule("generic", 0, Match::Always),
                rule("resolve", 50, Match::Running { process: "resolve.exe".into() }),
            ],
        };
        let w = world("chrome.exe", &running);
        assert_eq!(active_rule(&config, &w), Some(1));
    }

    #[test]
    fn foreground_match_ignores_case() {
        let running = procs(&[]);
        let config = Config {
            enabled: true,
            rules: vec![rule("r", 1, Match::Foreground { process: "Resolve.EXE".into() })],
        };
        let w = world("resolve.exe", &running);
        assert_eq!(active_rule(&config, &w), Some(0));
    }

    #[test]
    fn not_running_blocks() {
        let running = procs(&["resolve.exe"]);
        let config = Config {
            enabled: true,
            rules: vec![rule("no-resolve", 1, Match::NotRunning { process: "resolve.exe".into() })],
        };
        let w = world("chrome.exe", &running);
        assert_eq!(active_rule(&config, &w), None);
    }

    #[test]
    fn disabled_config_disables_everything() {
        let running = procs(&[]);
        let config = Config {
            enabled: false,
            rules: vec![rule("always", 99, Match::Always)],
        };
        let w = world("x.exe", &running);
        assert_eq!(active_rule(&config, &w), None);
    }

    #[test]
    fn json_roundtrip_and_sort() {
        let text = r#"{
            "enabled": true,
            "rules": [
                {"name":"low","priority":0,"match":{"type":"always"},"bindings":[]},
                {"name":"high","priority":10,"match":{"type":"foreground","process":"resolve.exe"},
                 "bindings":[{"input":"rotate_cw","action":{"type":"keys","keys":["j"]}}]}
            ]
        }"#;
        let config = Config::from_json(text).unwrap();
        assert_eq!(config.rules[0].name, "high");
        assert_eq!(config.rules[0].bindings[0].input.id(), "rotate_cw");
    }

    #[test]
    fn validate_rejects_unrunnable_bindings() {
        let parse = |action: &str| {
            Config::from_json(&format!(
                r#"{{"rules":[{{"name":"r","match":{{"type":"always"}},
                   "bindings":[{{"input":"rotate_cw","action":{action}}}]}}]}}"#
            ))
            .unwrap()
            .validate()
        };
        assert!(parse(r#"{"type":"keys","keys":["ctrl","="]}"#).is_ok());
        assert!(parse(r#"{"type":"wheel","modifiers":["alt"]}"#).is_ok());
        assert!(parse(r#"{"type":"keys","keys":["ctrl","bogus"]}"#).unwrap_err().contains("bogus"));
        assert!(parse(r#"{"type":"keys","keys":[]}"#).is_err());
        assert!(parse(r#"{"type":"wheel","notches":0}"#).is_err());
        assert!(parse(r#"{"type":"wheel","modifiers":["j"]}"#).unwrap_err().contains("j"));
    }

    #[test]
    fn exe_suffix_is_optional() {
        let running = procs(&["resolve"]);
        let config = Config {
            rules: vec![
                rule("fg", 10, Match::Foreground { process: "Resolve.exe".into() }),
                rule("run", 5, Match::Running { process: "resolve.exe".into() }),
            ],
            ..Default::default()
        };
        assert_eq!(active_rule(&config, &world("resolve", &running)), Some(0));
        assert_eq!(active_rule(&config, &world("", &running)), Some(1));
        assert_eq!(active_rule(&config, &world("", &procs(&["resolve.exe"]))), Some(1));
        assert_eq!(active_rule(&config, &world("resolver", &procs(&["resolver"]))), None);
    }
}
