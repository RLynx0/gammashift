use std::sync::Arc;

use chrono::Local;
use tokio::time::sleep;

use crate::gamma_relay_interface::{GammaRelayInterfaceProxy, set_property};
use crate::message::{GammaRelayProperty, ShiftMessage, TargetValue};
use crate::{DEFAULT_BRIGHTNESS, DEFAULT_GAMMA, DEFAULT_TEMPERATURE};

#[cfg(test)]
mod tests;

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
