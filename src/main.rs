use clap::Parser;

use gammashift::{
    cli::{Cli, GammaShiftCommand},
    engine::start_engine,
    message::{Message, ShiftMessage},
    sender::send_message,
};

const SOCK_FILE: &str = "/run/user/1000/gammashift/gammashift.sock";

fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt().init();

    match cli.command {
        GammaShiftCommand::Start { .. } => {
            if let Err(e) = start_engine(SOCK_FILE) {
                tracing::error!("Error starting gammashift engine: {e}")
            }
        }
        GammaShiftCommand::Stop => {
            if let Err(e) = send_message(SOCK_FILE, &Message::Stop) {
                tracing::error!("Error sending message to gammashift engine: {e}")
            }
        }
        GammaShiftCommand::Shift(opts) => {
            if let Err(e) = send_message(SOCK_FILE, &Message::Shift(ShiftMessage::from(&opts))) {
                tracing::error!("Error sending message to gammashift engine: {e}")
            }
        }
    }
}
