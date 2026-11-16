use super::*;
use crate::message::{ShiftMessage, TargetValue as MessageTargetValue};

#[test]
fn parses_shift_ids_and_negative_relative_values() {
    for (args, expected_id) in [
        (vec!["gammashift", "shift", "brightness", "-0.1"], ""),
        (
            vec![
                "gammashift",
                "shift",
                "brightness",
                "-0.1",
                "--id",
                "example",
            ],
            "example",
        ),
        (
            vec!["gammashift", "shift", "brightness", "-0.1", "-i", "other"],
            "other",
        ),
    ] {
        let Cli {
            command: GammaShiftCommand::Shift(opts),
        } = Cli::try_parse_from(args).unwrap()
        else {
            panic!("expected a shift command");
        };
        let message = ShiftMessage::from(&opts);
        assert_eq!(message.id, expected_id);
        assert!(matches!(message.target, MessageTargetValue::Relative(v) if v == -0.1));
    }
}
