use super::*;
use GammaRelayProperty::{Brightness, Gamma, Temperature};
use TargetValue::{Absolute, Default, Linear, Relative, Undo};

#[test]
fn absolute_target_preserves_the_reference_for_undo() {
    let record = plan_shift(&Absolute(0.8), Brightness, 0.4, Some(0.6), None).unwrap();

    assert_eq!(
        record,
        ShiftRecord {
            reference: 0.6,
            target: 0.8
        }
    );
}

#[test]
fn relative_target_uses_actual_when_no_shift_is_running() {
    let record = plan_shift(&Relative(-0.1), Brightness, 0.5, None, None).unwrap();

    assert_eq!(
        record,
        ShiftRecord {
            reference: 0.5,
            target: 0.4
        }
    );
}

#[test]
fn relative_target_uses_the_in_flight_target() {
    let record = plan_shift(&Relative(500.0), Temperature, 4100.0, Some(4500.0), None).unwrap();

    assert_eq!(
        record,
        ShiftRecord {
            reference: 4500.0,
            target: 5000.0
        }
    );
}

#[test]
fn linear_target_uses_actual_when_no_shift_is_running() {
    let record = plan_shift(&Linear(0.5), Brightness, 0.4, None, None).unwrap();

    assert_eq!(
        record,
        ShiftRecord {
            reference: 0.4,
            target: 0.2
        }
    );
}

#[test]
fn linear_target_uses_the_in_flight_target() {
    let record = plan_shift(&Linear(0.5), Brightness, 0.4, Some(0.6), None).unwrap();

    assert_eq!(
        record,
        ShiftRecord {
            reference: 0.6,
            target: 0.3
        }
    );
}

#[test]
fn default_target_uses_the_property_default_and_preserves_the_reference() {
    for (property, target) in [
        (Temperature, DEFAULT_TEMPERATURE),
        (Brightness, DEFAULT_BRIGHTNESS),
        (Gamma, DEFAULT_GAMMA),
    ] {
        let record = plan_shift(&Default, property, 0.4, Some(0.6), None).unwrap();

        assert_eq!(
            record,
            ShiftRecord {
                reference: 0.6,
                target
            },
            "{property}"
        );
    }
}

#[test]
fn undo_without_history_returns_an_error() {
    let result = plan_shift(&Undo, Temperature, 4200.0, Some(5000.0), None);

    assert_eq!(result, Err("nothing to undo"));
}

#[test]
fn undo_restores_the_stored_reference_not_the_previous_target() {
    let last = ShiftRecord {
        reference: 4500.0,
        target: 5000.0,
    };

    let record = plan_shift(&Undo, Temperature, 4200.0, Some(5000.0), Some(&last)).unwrap();

    assert_eq!(
        record,
        ShiftRecord {
            reference: 5000.0,
            target: 4500.0
        }
    );
}

#[test]
fn undo_after_completion_records_actual_as_reference() {
    let last = ShiftRecord {
        reference: 4500.0,
        target: 5000.0,
    };

    let record = plan_shift(&Undo, Temperature, 4900.0, None, Some(&last)).unwrap();

    assert_eq!(
        record,
        ShiftRecord {
            reference: 4900.0,
            target: 4500.0
        }
    );
}

#[test]
fn repeated_undo_toggles_between_the_previous_reference_and_target() {
    let mut last = Some(ShiftRecord {
        reference: 4500.0,
        target: 5000.0,
    });
    for (in_flight, expected) in [(5000.0, 4500.0), (4500.0, 5000.0), (5000.0, 4500.0)] {
        let record =
            plan_shift(&Undo, Temperature, 4200.0, Some(in_flight), last.as_ref()).unwrap();

        assert_eq!(
            record,
            ShiftRecord {
                reference: in_flight,
                target: expected
            }
        );
        last = Some(record);
    }
}
