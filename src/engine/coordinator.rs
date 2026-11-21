use std::{collections::HashMap, path::Path, sync::Arc};

use tokio::{
    sync::{Notify, mpsc::Receiver, oneshot},
    task::JoinHandle,
};

use super::{
    UnixSignals,
    shift::{ShiftId, ShiftRecord, plan_shift, run_ramp},
};
use crate::{
    MAX_BRIGHTNESS, MAX_GAMMA, MAX_TEMPERATURE, MIN_BRIGHTNESS, MIN_GAMMA, MIN_TEMPERATURE,
    gamma_relay_interface::{GammaRelayInterfaceProxy, get_property},
    message::{GammaRelayProperty, GetMessage, ShiftMessage, TargetValue},
};

#[cfg(test)]
mod tests;

pub(super) enum Instruction {
    Shift {
        message: ShiftMessage,
        reply: oneshot::Sender<Result<String, String>>,
    },
    Get {
        message: GetMessage,
        reply: oneshot::Sender<Result<String, String>>,
    },
    Stop {
        reply: oneshot::Sender<Result<String, String>>,
    },
}

struct InFlight {
    target: f64,
    handle: JoinHandle<()>,
}

struct Coordinator {
    proxy: GammaRelayInterfaceProxy<'static>,
    in_flight: HashMap<GammaRelayProperty, InFlight>,
    history: HashMap<ShiftId, ShiftRecord>,
}

impl Coordinator {
    async fn shift(&mut self, message: ShiftMessage) -> Result<String, String> {
        let property = message.property;
        let key = ShiftId {
            property,
            id: Arc::from(message.id.as_str()),
        };
        let last = self.history.get(&key).copied();
        if matches!(message.target, TargetValue::Undo) && last.is_none() {
            return Err(format!(
                "Nothing to undo for {property} (id {:?})",
                message.id
            ));
        }

        let mut in_flight_target = None;
        if let Some(in_flight) = self.in_flight.remove(&property) {
            if !in_flight.handle.is_finished() {
                in_flight_target = Some(in_flight.target);
                in_flight.handle.abort();
            }
            let _ = in_flight.handle.await;
        }
        let actual = match get_property(&self.proxy, &property).await {
            Ok(value) => value,
            Err(e) => return Err(format!("Error fetching current {property}: {e}")),
        };
        let record = match plan_shift(
            &message.target,
            property,
            actual,
            in_flight_target,
            last.as_ref(),
        ) {
            Ok(record) => record,
            Err(e) => return Err(format!("{e} for {property} (id {:?})", message.id)),
        };
        let duration = message.end - message.start;
        let response = format!(
            "Shifting {property}: {actual} -> {} ({:.3}s)",
            record.target,
            duration.as_seconds_f64()
        );
        self.history.insert(key, record);
        let handle = tokio::spawn(run_ramp(self.proxy.clone(), message, actual, record.target));
        self.in_flight.insert(
            property,
            InFlight {
                target: record.target,
                handle,
            },
        );
        tracing::info!("{response}");
        Ok(response)
    }

    async fn get(&self, message: GetMessage) -> Result<String, String> {
        let property = message.property;
        let mut in_flight_target = None;
        if let Some(in_flight) = self.in_flight.get(&property)
            && !in_flight.handle.is_finished()
        {
            in_flight_target = Some(in_flight.target);
        }
        let actual = match get_property(&self.proxy, &property).await {
            Ok(value) => value,
            Err(e) => return Err(format!("Error fetching current {property}: {e}")),
        };
        let reference = in_flight_target.unwrap_or(actual);
        let reference = match (message.proportion, message.property) {
            (true, GammaRelayProperty::Temperature) => {
                (reference - MIN_TEMPERATURE) / (MAX_TEMPERATURE - MIN_TEMPERATURE)
            }
            (true, GammaRelayProperty::Brightness) => {
                (reference - MIN_BRIGHTNESS) / (MAX_BRIGHTNESS - MIN_BRIGHTNESS)
            }
            (true, GammaRelayProperty::Gamma) => (reference - MIN_GAMMA) / (MAX_GAMMA - MIN_GAMMA),
            (false, _) => reference,
        };
        Ok(format!("{reference:.0$}", message.round as usize))
    }

    async fn stop(&mut self) {
        for (_, in_flight) in self.in_flight.drain() {
            in_flight.handle.abort();
            let _ = in_flight.handle.await;
        }
    }
}

pub(super) async fn start_coordinator(
    mut receiver: Receiver<Instruction>,
    mut unix_signals: UnixSignals,
    shutdown: Arc<Notify>,
    socket_path: Arc<Path>,
    proxy: GammaRelayInterfaceProxy<'static>,
) {
    let mut coordinator = Coordinator {
        proxy,
        in_flight: HashMap::new(),
        history: HashMap::new(),
    };
    loop {
        tokio::select! {
            inst = receiver.recv() => {
                match inst {
                    Some(Instruction::Shift { message, reply }) => {
                        match coordinator.shift(message).await {
                            Ok(response) => _ = reply.send(Ok(response)),
                            Err(response) => {
                                tracing::error!("{response}");
                                _ = reply.send(Err(response));
                            },
                        }
                    }
                    Some(Instruction::Get { message, reply }) => {
                        match coordinator.get(message).await {
                            Ok(response) => _ = reply.send(Ok(response)),
                            Err(response) => {
                                tracing::error!("{response}");
                                _ = reply.send(Err(response));
                            }
                        }
                    }
                    Some(Instruction::Stop { reply }) => {
                        _ = reply.send(Ok(String::from("Shutting down")));
                        break;
                    }
                    None => break,
                }
            }
            _ = unix_signals.sigint.recv() => {
                tracing::info!("Received SIGINT");
                break;
            }
            _ = unix_signals.sigterm.recv() => {
                tracing::info!("Received SIGTERM");
                break;
            }
        }
    }
    coordinator.stop().await;
    handle_stop(&shutdown, &socket_path).await;
}

async fn handle_stop(shutdown: &Arc<Notify>, sock_path: impl AsRef<Path>) {
    tracing::info!("Shutting down");
    if let Err(e) = tokio::fs::remove_file(sock_path).await {
        tracing::error!("Error removing socket: {e}");
    }
    shutdown.notify_waiters();
}
