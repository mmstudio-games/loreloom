mod bridge;
mod cli;
mod client;
mod demo;
mod error;

use std::process::ExitCode;

use cli::{Cli, HELP};
use client::RuntimeAdapter;
use demo::{DemoSetup, build_demo};
use error::AppError;

fn main() -> ExitCode {
    match run_application() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("loreloom: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_application() -> Result<(), AppError> {
    let cli = Cli::parse(std::env::args_os())?;
    if cli.help {
        print!("{HELP}");
        return Ok(());
    }
    let tokio = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("loreloom-io")
        .build()
        .map_err(AppError::Tokio)?;
    let DemoSetup {
        mut runtime,
        initial_snapshot,
    } = tokio.block_on(build_demo(&cli.save_path))?;
    if let Some(input) = cli.headless_input {
        let outcome = tokio.block_on(runtime.handle_player_input(input))?;
        println!("{}", outcome.narration);
        println!("revision {}", outcome.snapshot.revision);
        return Ok(());
    }

    let mut client = RuntimeAdapter::spawn(runtime)?;
    loreloom_tui::run(
        &mut client,
        initial_snapshot,
        loreloom_tui::TuiConfig::default(),
    )?;
    Ok(())
}
