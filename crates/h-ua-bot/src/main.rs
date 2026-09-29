// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! ```text
//! h-ua-bot [run]
//! ```
//!
//! Starts the bot from `HUA_*` environment variables (see `.env.example`): it talks to people on
//! Telegram, keeps their subscriptions in `prism-hub`, and sends what the hub delivers.

use std::process::ExitCode;

use h_ua_bot::config::Config;
use h_ua_bot::runtime;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: h-ua-bot [run]";

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        None | Some("run") if args.len() <= 1 => run().await,
        Some("-h" | "--help") => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let config = Config::from_env().map_err(|e| e.to_string())?;
    runtime::run(config).await
}
