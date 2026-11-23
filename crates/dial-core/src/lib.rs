//! Core types and pure logic for app-aware dial mappings.
//! No I/O, no Windows dependencies — safe to test anywhere.

pub mod config;
pub mod input;
pub mod keys;
pub mod rules;

pub use config::{Action, Binding, Config, Match, Rule};
pub use input::InputId;
pub use keys::{Key, Named, parse_chord, parse_key, parse_modifiers};
pub use rules::{World, active_rule};
