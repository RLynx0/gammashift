use std::{fmt::Display, time::Duration};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::cli;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Shift(ShiftMessage),
    Get(GetMessage),
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "message", rename_all = "lowercase")]
pub enum EngineResponse {
    Ok(String),
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftMessage {
    pub property: GammaRelayProperty,
    #[serde(default)]
    pub id: String,
    pub target: TargetValue,
    pub step_duration: Duration,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
}
impl From<&cli::ShiftOptions> for ShiftMessage {
    fn from(opts: &cli::ShiftOptions) -> Self {
        let start = Local::now();
        let end = match opts.time {
            cli::ShiftTime::Instant => start,
            cli::ShiftTime::Dur(d) => start + d,
        };
        ShiftMessage {
            id: opts.id.clone(),
            property: match opts.property {
                cli::GammaRelayProperty::Temperature => GammaRelayProperty::Temperature,
                cli::GammaRelayProperty::Brightness => GammaRelayProperty::Brightness,
                cli::GammaRelayProperty::Gamma => GammaRelayProperty::Gamma,
            },
            target: match opts.target_value {
                cli::TargetValue::Undo => TargetValue::Undo,
                cli::TargetValue::Default => TargetValue::Default,
                cli::TargetValue::Absolute(val) => TargetValue::Absolute(val),
                cli::TargetValue::Relative(val) => TargetValue::Relative(val),
                cli::TargetValue::Linear(val) => TargetValue::Linear(val),
            },
            step_duration: Duration::from_millis(10),
            start,
            end,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetMessage {
    pub property: GammaRelayProperty,
    pub proportion: bool,
    pub round: u8,
}
impl From<&cli::GetOptions> for GetMessage {
    fn from(opts: &cli::GetOptions) -> Self {
        GetMessage {
            round: opts.round,
            proportion: opts.proportion,
            property: match opts.property {
                cli::GammaRelayProperty::Temperature => GammaRelayProperty::Temperature,
                cli::GammaRelayProperty::Brightness => GammaRelayProperty::Brightness,
                cli::GammaRelayProperty::Gamma => GammaRelayProperty::Gamma,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GammaRelayProperty {
    Temperature,
    Brightness,
    Gamma,
}
impl Display for GammaRelayProperty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GammaRelayProperty::Temperature => write!(f, "Temperature"),
            GammaRelayProperty::Brightness => write!(f, "Brightness"),
            GammaRelayProperty::Gamma => write!(f, "Gamma"),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TargetValue {
    Undo,
    Default,
    Absolute(f64),
    Relative(f64),
    Linear(f64),
}
