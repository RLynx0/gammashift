use super::*;
use crate::message::{ShiftMessage, TargetValue as MessageTargetValue};

fn parse_shift(args: &[&str]) -> ShiftMessage {
    let args = ["gammashift", "shift", "brightness"]
        .into_iter()
        .chain(args.iter().copied());
    let Cli {
        command: GammaShiftCommand::Shift(opts),
    } = Cli::try_parse_from(args).unwrap()
    else {
        panic!("expected a shift command");
    };
    ShiftMessage::from(&opts)
}

#[test]
fn shift_id_defaults_to_empty() {
    let message = parse_shift(&["0.5"]);

    assert_eq!(message.id, "");
}

#[test]
fn shift_accepts_long_id_option() {
    let message = parse_shift(&["0.5", "--id", "example"]);

    assert_eq!(message.id, "example");
}

#[test]
fn shift_accepts_short_id_option() {
    let message = parse_shift(&["0.5", "-i", "example"]);

    assert_eq!(message.id, "example");
}

#[test]
fn shift_accepts_negative_relative_target_without_option_separator() {
    let message = parse_shift(&["-0.1"]);

    assert!(matches!(message.target, MessageTargetValue::Relative(v) if v == -0.1));
}

#[test]
fn shift_accepts_positive_relative_target() {
    let message = parse_shift(&["+0.1"]);

    assert!(matches!(message.target, MessageTargetValue::Relative(v) if v == 0.1));
}

#[test]
fn shift_accepts_linear_target() {
    let message = parse_shift(&["*0.5"]);

    assert!(matches!(message.target, MessageTargetValue::Linear(v) if v == 0.5));
}

#[test]
fn shift_accepts_absolute_target() {
    let message = parse_shift(&["0.5"]);

    assert!(matches!(message.target, MessageTargetValue::Absolute(v) if v == 0.5));
}

#[test]
fn shift_accepts_undo_target() {
    let message = parse_shift(&["undo", "--id", "example"]);

    assert!(matches!(message.target, MessageTargetValue::Undo));
    assert_eq!(message.id, "example");
}

#[test]
fn shift_accepts_default_target() {
    let message = parse_shift(&["default"]);

    assert!(matches!(message.target, MessageTargetValue::Default));
}

#[test]
fn shift_rejects_invalid_target() {
    let result = Cli::try_parse_from(["gammashift", "shift", "brightness", "invalid"]);

    assert_eq!(
        result.unwrap_err().kind(),
        clap::error::ErrorKind::ValueValidation
    );
}
