use std::{num::ParseFloatError, str::FromStr, time::Duration};

use clap::{Parser, Subcommand, ValueEnum};

/// Transition engine for ws-gamma-relay-rs
#[derive(Debug, Clone, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: GammaShiftCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum GammaShiftCommand {
    /// Start the gammashift engine
    #[command(author, version, about, long_about = None)]
    Start {
        /// Start in daemon mode
        #[arg(short, long)]
        daemon: bool,
    },

    Msg(MsgOpts),
}

/// Send an instruction to a running gammashift engine
#[derive(Debug, Clone, Parser)]
#[command(author, version, about, long_about = None)]
pub struct MsgOpts {
    /// Property to shift.
    pub property: GammaRelayProperty,

    /// Target value to shift the property to.
    pub target_value: TargetValue,

    /// Duration after which the target value should be reached.
    ///
    /// Must be a positive float, optionally followed by a prefix.
    /// Without a prefix, the value is treated as representing seconds.
    ///
    /// Supported prefixes:
    /// - ms | milliseconds
    /// - s  | seconds
    /// - m  | minutes
    ///
    /// Examples:
    /// - 0      Immediate
    /// - 2s     Two seconds
    /// - 1.5    One and a half seconds
    /// - 0.5m   Half a minute
    /// - 200ms  200 milliseconds
    #[arg(short, long, default_value = "2.0 seconds", verbatim_doc_comment)]
    pub time: ShiftTime,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum GammaRelayProperty {
    Temperature,
    Brightness,
    Gamma,
}

#[derive(Debug, Clone, Copy)]
pub enum TargetValue {
    Undo,
    Default,
    Absolute(f64),
    Relative(f64),
}
impl FromStr for TargetValue {
    type Err = ParseFloatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "undo" => Ok(TargetValue::Undo),
            "default" => Ok(TargetValue::Default),
            relative_str if matches!(relative_str.chars().next(), Some('+') | Some('-')) => {
                relative_str.parse::<f64>().map(TargetValue::Relative)
            }
            absolute_str => absolute_str.parse::<f64>().map(TargetValue::Absolute),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ShiftTime {
    Instant,
    Dur(Duration),
}
impl FromStr for ShiftTime {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s_chars = s.chars();
        let number_part = s_chars
            .take_while(|c| c.is_numeric() || ['.', '-', '+'].contains(c))
            .collect::<String>();
        if number_part == "0" {
            return Ok(ShiftTime::Instant);
        }
        let number_value = number_part.parse::<f64>().map_err(|e| e.to_string())?;
        if number_value < 0.0 {
            return Err("Shift duration cannot be less than 0".to_string());
        }

        match s[number_part.len()..].trim() {
            "ms" | "milliseconds" => Ok(ShiftTime::Dur(Duration::from_millis(number_value as u64))),
            "s" | "seconds" | "" => Ok(ShiftTime::Dur(Duration::from_secs_f64(number_value))),
            "m" | "minutes" => Ok(ShiftTime::Dur(Duration::from_secs_f64(number_value * 60.0))),
            unknown => Err(format!("Unsupported duration prefix '{unknown}'")),
        }
    }
}
