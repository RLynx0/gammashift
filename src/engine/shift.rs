use std::sync::Arc;

use chrono::Local;
use tokio::time::sleep;

use crate::gamma_relay_interface::{GammaRelayInterfaceProxy, set_property};
use crate::message::{GammaRelayProperty, ShiftMessage, TargetValue};
use crate::{DEFAULT_BRIGHTNESS, DEFAULT_GAMMA, DEFAULT_TEMPERATURE};

#[derive(Debug, PartialEq, Eq, Hash)]
pub(super) struct ShiftId {
    pub property: GammaRelayProperty,
    pub id: Arc<str>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ShiftRecord {
    pub reference: f64,
    pub target: f64,
}

pub(super) fn plan_shift(
    target: &TargetValue,
    property: GammaRelayProperty,
    actual: f64,
    in_flight_target: Option<f64>,
    last: Option<&ShiftRecord>,
) -> Result<ShiftRecord, &'static str> {
    let reference = in_flight_target.unwrap_or(actual);
    let target = match target {
        TargetValue::Absolute(value) => *value,
        TargetValue::Relative(delta) => reference + delta,
        TargetValue::Linear(factor) => reference * factor,
        TargetValue::Undo => last.ok_or("nothing to undo")?.reference,
        TargetValue::Default => match property {
            GammaRelayProperty::Temperature => DEFAULT_TEMPERATURE,
            GammaRelayProperty::Brightness => DEFAULT_BRIGHTNESS,
            GammaRelayProperty::Gamma => DEFAULT_GAMMA,
        },
    };
    Ok(ShiftRecord { reference, target })
}

pub(super) async fn run_ramp(
    proxy: GammaRelayInterfaceProxy<'_>,
    message: ShiftMessage,
    actual: f64,
    target: f64,
) {
    let duration = (message.end - message.start).as_seconds_f64();
    loop {
        let now = Local::now();
        let finished = now >= message.end || duration <= 0.0;
        let step_target = if finished {
            target
        } else {
            let factor = ((now - message.start).as_seconds_f64() / duration).clamp(0.0, 1.0);
            actual + factor * (target - actual)
        };
        if let Err(e) = set_property(&proxy, &message.property, step_target).await {
            return tracing::error!("Failed to set property: {e}");
        }
        if finished {
            return tracing::info!("Shifted {}: {target}", message.property);
        }
        tracing::debug!("Shiftstep {}:{step_target}", message.property);
        sleep(message.step_duration).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use GammaRelayProperty::*;
    use TargetValue::*;

    #[test]
    fn interrupted_relative_shifts_then_undo_toggle_between_targets() {
        let mut last = None;
        for (target, actual, in_flight, reference, expected) in [
            (Relative(500.0), 4000.0, None, 4000.0, 4500.0),
            (Relative(500.0), 4100.0, Some(4500.0), 4500.0, 5000.0),
            (Undo, 4200.0, Some(5000.0), 5000.0, 4500.0),
            (Undo, 4300.0, Some(4500.0), 4500.0, 5000.0),
            (Undo, 4400.0, Some(5000.0), 5000.0, 4500.0),
        ] {
            let record =
                plan_shift(&target, Temperature, actual, in_flight, last.as_ref()).unwrap();
            assert_eq!(
                record,
                ShiftRecord {
                    reference,
                    target: expected
                }
            );
            last = Some(record);
        }
    }

    #[test]
    fn undo_after_completion_records_actual_as_reference() {
        let last = ShiftRecord {
            reference: 4500.0,
            target: 5000.0,
        };
        let undo = plan_shift(&Undo, Temperature, 5000.0, None, Some(&last)).unwrap();
        assert_eq!(
            undo,
            ShiftRecord {
                reference: 5000.0,
                target: 4500.0
            }
        );
        let redo = plan_shift(&Undo, Temperature, 4500.0, None, Some(&undo)).unwrap();
        assert_eq!(redo.target, 5000.0);
        assert_eq!(
            plan_shift(&Undo, Temperature, 4500.0, None, None),
            Err("nothing to undo")
        );
    }

    #[test]
    fn target_modes_use_the_correct_reference() {
        for (target, in_flight, expected) in [
            (Absolute(0.8), Some(0.6), 0.8),
            (Relative(0.1), None, 0.5),
            (Linear(0.5), None, 0.2),
            (Linear(0.5), Some(0.6), 0.3),
        ] {
            let record = plan_shift(&target, Brightness, 0.4, in_flight, None).unwrap();
            assert_eq!(record.target, expected);
            assert_eq!(record.reference, in_flight.unwrap_or(0.4));
        }
        for (property, expected) in [
            (Temperature, DEFAULT_TEMPERATURE),
            (Brightness, DEFAULT_BRIGHTNESS),
            (Gamma, DEFAULT_GAMMA),
        ] {
            assert_eq!(
                plan_shift(&Default, property, 0.4, None, None)
                    .unwrap()
                    .target,
                expected
            );
        }
    }
}
