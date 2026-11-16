use std::process::ExitCode;

use clap::Parser;

use gammashift::{
    cli::{Cli, GammaShiftCommand},
    engine::start_engine,
    message::{Message, ShiftMessage},
    sender::send_message,
    socket_path,
};

fn main() -> ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt().init();
    let socket = match socket_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    let result = match cli.command {
        GammaShiftCommand::Start => start_engine(socket).map(|()| None),
        GammaShiftCommand::Stop => send_message(socket, &Message::Stop).map(Some),
        GammaShiftCommand::Shift(opts) => {
            send_message(socket, &Message::Shift(ShiftMessage::from(&opts))).map(Some)
        }
    };

    match result {
        Ok(Some(response)) => {
            println!("{response}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
