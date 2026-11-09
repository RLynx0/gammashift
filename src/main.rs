use std::{fmt::Display, path::PathBuf, time::Duration};

use chrono::{DateTime, Local};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    time::sleep,
};
use zbus::proxy;

use crate::cli::{Cli, MsgOpts};

mod cli;

const SOCK_FILE: &str = "/run/user/1000/gammashift/gammashift.sock";

const DEFAULT_TEMPERATURE: f64 = 6500.0;
const DEFAULT_BRIGHTNESS: f64 = 1.0;
const DEFAULT_GAMMA: f64 = 1.0;

fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt().init();

    match cli.command {
        cli::GammaShiftCommand::Start { .. } => {
            if let Err(e) = start_engine() {
                tracing::error!("Error starting gammashift engine: {e}")
            }
        }
        cli::GammaShiftCommand::Msg(msg) => {
            if let Err(e) = send_message(&msg) {
                tracing::error!("Error sending message to gammashift engine: {e}")
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    property: GammaRelayProperty,
    target: TargetValue,
    step_duration: Duration,
    start: DateTime<Local>,
    end: DateTime<Local>,
}
impl From<&MsgOpts> for Message {
    fn from(opts: &MsgOpts) -> Self {
        let start = Local::now();
        let end = match opts.time {
            cli::ShiftTime::Instant => start,
            cli::ShiftTime::Dur(d) => start + d,
        };
        Message {
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
            },
            step_duration: Duration::from_millis(2),
            start,
            end,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
enum GammaRelayProperty {
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
enum TargetValue {
    Undo,
    Default,
    Absolute(f64),
    Relative(f64),
}

#[tokio::main]
async fn send_message(opts: &MsgOpts) -> tokio::io::Result<()> {
    let stream = UnixStream::connect(SOCK_FILE).await?;
    let (reader, mut writer) = stream.into_split();
    let data = serde_json::to_vec(&Message::from(opts))?;
    writer.write_all(&data).await?;
    drop(writer);

    let mut reader = BufReader::new(reader);
    let mut response = String::new();
    reader.read_line(&mut response).await?;
    drop(reader);

    println!("{response}");
    Ok(())
}

#[tokio::main]
async fn start_engine() -> tokio::io::Result<()> {
    let path = PathBuf::from(SOCK_FILE);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(&dir).await?;
    }
    let listener = UnixListener::bind(SOCK_FILE)?;
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                tracing::error!("Error accepting from unix listener: {e}");
                continue;
            }
        };

        tokio::spawn(async move {
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);

            let mut message = String::new();
            if let Err(e) = reader.read_line(&mut message).await {
                return tracing::error!("Error reading stream: {e}");
            }
            let Message {
                property,
                target,
                step_duration,
                start: start_time,
                end: end_time,
            } = match serde_json::from_str(&message) {
                Err(e) => return tracing::error!("Error decoding message: {e}"),
                Ok(decoded) => decoded,
            };

            let dbus_connection = match zbus::Connection::session().await {
                Err(e) => return tracing::error!("Error establishing d-bus connection: {e}"),
                Ok(connection) => connection,
            };
            let gammarelay_proxy = match GammaRelayInterfaceProxy::new(&dbus_connection).await {
                Err(e) => return tracing::error!("Error setting up d-bus proxy: {e}"),
                Ok(proxy) => proxy,
            };

            let current = match get_property(&gammarelay_proxy, &property).await {
                Err(e) => return tracing::error!("Failed fetching current value: {e}"),
                Ok(val) => val,
            };

            let target = match target {
                TargetValue::Absolute(v) => v,
                TargetValue::Relative(r) => current + r,
                TargetValue::Undo => todo!("Implement Undo"),
                TargetValue::Default => match &property {
                    GammaRelayProperty::Temperature => DEFAULT_TEMPERATURE,
                    GammaRelayProperty::Brightness => DEFAULT_BRIGHTNESS,
                    GammaRelayProperty::Gamma => DEFAULT_GAMMA,
                },
            };
            let duration = Duration::from_secs_f64((end_time - start_time).as_seconds_f64());
            let response = format!("[STARTED] {property}: {current} -> {target} ({duration:?})");
            if let Err(e) = writer.write_all(response.as_bytes()).await {
                return tracing::error!("Error writing to stream: {e}");
            }
            drop(writer);
            tracing::info!("{response}");

            loop {
                let now = Local::now();
                if now > end_time {
                    match set_property(&gammarelay_proxy, &property, target).await {
                        Err(e) => return tracing::error!("Failed to set property: {e}"),
                        Ok(_) => return tracing::info!("[DONE] {property}:{target}"),
                    }
                }
                let elapsed = now - start_time;
                let factor = elapsed.as_seconds_f64() / duration.as_secs_f64();
                let step_target = current + factor * (target - current);
                match set_property(&gammarelay_proxy, &property, step_target).await {
                    Err(e) => return tracing::error!("Failed to set property: {e}"),
                    Ok(_) => tracing::debug!("[STEP] {property}:{step_target}"),
                }
                sleep(step_duration).await;
            }
        });
    }
}

#[proxy(
    interface = "rs.wl.gammarelay",
    default_service = "rs.wl-gammarelay",
    default_path = "/"
)]
trait GammaRelayInterface {
    #[zbus(property)]
    fn temperature(&self) -> zbus::Result<u16>;
    #[zbus(property)]
    fn set_temperature(&self, value: u16) -> zbus::Result<()>;

    #[zbus(property)]
    fn gamma(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn set_gamma(&self, value: f64) -> zbus::Result<()>;

    #[zbus(property)]
    fn brightness(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn set_brightness(&self, value: f64) -> zbus::Result<()>;
}

async fn set_property<'a>(
    gamma_relay_prox: &GammaRelayInterfaceProxy<'a>,
    property: &GammaRelayProperty,
    value: f64,
) -> zbus::Result<()> {
    match property {
        GammaRelayProperty::Temperature => gamma_relay_prox.set_temperature(value as u16).await,
        GammaRelayProperty::Brightness => gamma_relay_prox.set_brightness(value).await,
        GammaRelayProperty::Gamma => gamma_relay_prox.set_gamma(value).await,
    }
}
async fn get_property<'a>(
    gamma_relay_prox: &GammaRelayInterfaceProxy<'a>,
    property: &GammaRelayProperty,
) -> zbus::Result<f64> {
    match property {
        GammaRelayProperty::Temperature => gamma_relay_prox.temperature().await.map(|t| t as f64),
        GammaRelayProperty::Brightness => gamma_relay_prox.brightness().await,
        GammaRelayProperty::Gamma => gamma_relay_prox.gamma().await,
    }
}
