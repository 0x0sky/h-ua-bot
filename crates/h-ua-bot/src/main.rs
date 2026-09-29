// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! ```text
//! h-ua-bot [run]
//! h-ua-bot simulate --lat <deg> --lon <deg> [--kinds drone,bomb,missile] [--no-nearby] < posts.ndjson
//! ```
//!
//! `run` starts the bot from `HUA_*` environment variables (see `.env.example`). `simulate`
//! replays `Evidence` NDJSON, as `prism-signal-collect` prints it, for one imaginary person and
//! prints what they would have been told.

use std::collections::BTreeSet;
use std::io::BufRead;
use std::process::ExitCode;

use h_ua_bot::config::Config;
use h_ua_bot::runtime;
use h_ua_bot::simulate::{Person, simulate};
use h_ua_core::category::Category;
use prism_signal_core::Evidence;
use prism_signal_normalize::Normalizer;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage:
  h-ua-bot [run]
  h-ua-bot simulate --lat <deg> --lon <deg> [--kinds drone,bomb,missile] [--no-nearby] < posts.ndjson";

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
        Some("simulate") => simulate_command(&args[1..]).await,
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

fn parse_person(args: &[String]) -> Result<Person, String> {
    let (mut lat, mut lon) = (None, None);
    let mut categories = None;
    let mut include_nearby = true;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let mut value = || iter.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--lat" => {
                lat = Some(
                    value()?
                        .parse::<f64>()
                        .map_err(|_| "--lat must be a number")?,
                )
            }
            "--lon" => {
                lon = Some(
                    value()?
                        .parse::<f64>()
                        .map_err(|_| "--lon must be a number")?,
                )
            }
            "--kinds" => {
                let chosen: Result<BTreeSet<Category>, String> = value()?
                    .split(',')
                    .map(|word| {
                        Category::from_word(word).ok_or_else(|| format!("unknown kind `{word}`"))
                    })
                    .collect();
                categories = Some(chosen?);
            }
            "--no-nearby" => include_nearby = false,
            other => return Err(format!("unknown option `{other}`\n{USAGE}")),
        }
    }
    Ok(Person {
        lat: lat.ok_or("--lat is required")?,
        lon: lon.ok_or("--lon is required")?,
        categories,
        include_nearby,
    })
}

async fn simulate_command(args: &[String]) -> Result<(), String> {
    let person = parse_person(args)?;
    let mut posts = Vec::new();
    for (number, line) in std::io::stdin().lock().lines().enumerate() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        posts.push(
            serde_json::from_str::<Evidence>(&line)
                .map_err(|e| format!("line {}: {e}", number + 1))?,
        );
    }
    posts.sort_by_key(|post| post.published_at);
    let normalizer = Normalizer::embedded().map_err(|e| e.to_string())?;
    let received = simulate(&person, &posts, &normalizer)
        .await
        .map_err(|_| "those coordinates are not valid".to_owned())?;
    for message in &received {
        println!("── {} ──\n{}\n", message.at, message.text);
    }
    eprintln!("{} posts, {} messages", posts.len(), received.len());
    Ok(())
}
