use std::path::PathBuf;

use chrono::{DateTime, Local};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

use crate::cli::{Cli, MsgOpts};

mod cli;

const SOCK_FILE: &str = "/run/user/1000/gammashift/gammashift.sock";

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

#[derive(Serialize, Deserialize)]
struct Message {
    property: &'static str,
    target: TargetValue,
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
                cli::GammaRelayProperty::Temperature => "Temperature",
                cli::GammaRelayProperty::Brightness => "Brightness",
                cli::GammaRelayProperty::Gamma => "Gamma",
            },
            target: match opts.target_value {
                cli::TargetValue::Undo => TargetValue::Undo,
                cli::TargetValue::Absolute(val) => TargetValue::Absolute(val),
                cli::TargetValue::Relative(val) => TargetValue::Relative(val),
            },
            start,
            end,
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum TargetValue {
    Undo,
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

            tracing::info!("Received message: {message}");
            if let Err(e) = writer.write_all(b"Hello from daemon").await {
                tracing::error!("Error writing to stream: {e}")
            }
        });
    }
}
