use std::time::Duration;

use chrono::Local;
use tokio::{io::AsyncWriteExt, net::unix::OwnedWriteHalf, time::sleep};

use crate::gamma_relay_interface::{GammaRelayInterfaceProxy, get_property, set_property};
use crate::message::{GammaRelayProperty, ShiftMessage, TargetValue};
use crate::{DEFAULT_BRIGHTNESS, DEFAULT_GAMMA, DEFAULT_TEMPERATURE};

pub(crate) async fn handle_shift(
    ShiftMessage {
        property,
        target,
        step_duration,
        start: start_time,
        end: end_time,
    }: &ShiftMessage,
    mut writer: OwnedWriteHalf,
) {
    let step_duration = *step_duration;
    let start_time = *start_time;
    let end_time = *end_time;

    let dbus_connection = match zbus::Connection::session().await {
        Err(e) => return tracing::error!("Error establishing d-bus connection: {e}"),
        Ok(connection) => connection,
    };
    let gammarelay_proxy = match GammaRelayInterfaceProxy::new(&dbus_connection).await {
        Err(e) => return tracing::error!("Error setting up d-bus proxy: {e}"),
        Ok(proxy) => proxy,
    };

    let current = match get_property(&gammarelay_proxy, property).await {
        Err(e) => return tracing::error!("Error fetching current value: {e}"),
        Ok(val) => val,
    };

    let target = match target {
        TargetValue::Absolute(value) => *value,
        TargetValue::Relative(delta) => current + delta,
        TargetValue::Linear(factor) => current * factor,
        TargetValue::Undo => todo!("Implement Undo"),
        TargetValue::Default => match &property {
            GammaRelayProperty::Temperature => DEFAULT_TEMPERATURE,
            GammaRelayProperty::Brightness => DEFAULT_BRIGHTNESS,
            GammaRelayProperty::Gamma => DEFAULT_GAMMA,
        },
    };
    let duration = Duration::from_secs_f64((end_time - start_time).as_seconds_f64());
    let response = format!("Shifting {property}: {current} -> {target} ({duration:?})");
    if let Err(e) = writer.write_all(response.as_bytes()).await {
        tracing::error!("Error writing to stream: {e}");
    }
    drop(writer);
    tracing::info!("{response}");

    loop {
        let now = Local::now();
        if now > end_time {
            match set_property(&gammarelay_proxy, property, target).await {
                Err(e) => return tracing::error!("Failed to set property: {e}"),
                Ok(_) => return tracing::info!("Shifted {property}: {target}"),
            }
        }
        let elapsed = now - start_time;
        let factor = elapsed.as_seconds_f64() / duration.as_secs_f64();
        let step_target = current + factor * (target - current);
        match set_property(&gammarelay_proxy, property, step_target).await {
            Err(e) => return tracing::error!("Failed to set property: {e}"),
            Ok(_) => tracing::debug!("Shiftstep {property}:{step_target}"),
        }
        sleep(step_duration).await;
    }
}
