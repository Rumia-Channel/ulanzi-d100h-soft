//! Physical dial inputs, as stable IDs for config and UI.
use serde::{Deserialize, Serialize};

/// Every input the D100H can produce.
/// `snake_case` names are the wire/config spelling, e.g. "rotate_held_cw".
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputId {
    ButtonTopLeft,
    ButtonTopMiddle,
    ButtonTopRight,
    ButtonUpperLeft,
    ButtonUpperRight,
    ButtonLowerLeft,
    ButtonLowerRight,
    KnobClick,
    RotateCw,
    RotateCcw,
    RotateHeldCw,
    RotateHeldCcw,
}

impl InputId {
    /// All inputs, in display order — used by the rule editor UI.
    pub const ALL: [Self; 12] = [
        Self::ButtonTopLeft,
        Self::ButtonTopMiddle,
        Self::ButtonTopRight,
        Self::ButtonUpperLeft,
        Self::ButtonUpperRight,
        Self::ButtonLowerLeft,
        Self::ButtonLowerRight,
        Self::KnobClick,
        Self::RotateCw,
        Self::RotateCcw,
        Self::RotateHeldCw,
        Self::RotateHeldCcw,
    ];

    /// Human-readable label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::ButtonTopLeft => "Button top-left",
            Self::ButtonTopMiddle => "Button top-middle",
            Self::ButtonTopRight => "Button top-right",
            Self::ButtonUpperLeft => "Button upper-left",
            Self::ButtonUpperRight => "Button upper-right",
            Self::ButtonLowerLeft => "Button lower-left",
            Self::ButtonLowerRight => "Button lower-right",
            Self::KnobClick => "Knob click",
            Self::RotateCw => "Rotate clockwise",
            Self::RotateCcw => "Rotate counter-clockwise",
            Self::RotateHeldCw => "Rotate held clockwise",
            Self::RotateHeldCcw => "Rotate held counter-clockwise",
        }
    }

    /// serde spelling used in config.json and JSON messages.
    pub fn id(self) -> &'static str {
        // serde(rename_all=snake_case) matches these literals exactly.
        match self {
            Self::ButtonTopLeft => "button_top_left",
            Self::ButtonTopMiddle => "button_top_middle",
            Self::ButtonTopRight => "button_top_right",
            Self::ButtonUpperLeft => "button_upper_left",
            Self::ButtonUpperRight => "button_upper_right",
            Self::ButtonLowerLeft => "button_lower_left",
            Self::ButtonLowerRight => "button_lower_right",
            Self::KnobClick => "knob_click",
            Self::RotateCw => "rotate_cw",
            Self::RotateCcw => "rotate_ccw",
            Self::RotateHeldCw => "rotate_held_cw",
            Self::RotateHeldCcw => "rotate_held_ccw",
        }
    }
}
