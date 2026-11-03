use std::process::ExitCode;

use agent_beacon::cli::{run, Cli};
use clap::Parser;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = run(cli).await;
    ExitCode::from(code as u8)
}
