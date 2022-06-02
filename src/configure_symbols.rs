use rock::config::{read_config, pull_symbols};
use rock::exchange::{Exchange, build_exchange};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::prelude::*;
use clap::Parser;
#[macro_use]
extern crate log;

/// Program prepares a list of symbols traded at each of exchanges enabled in config file
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Path to exchanges config file
    #[clap(short, long)]
    config_file: String,

    /// Path to symbols output file
    #[clap(short, long)]
    symbols_file: String,
}

fn main() {
    env_logger::init();
    let args = Args::parse();
    let exchanges_info = &read_config(&args.config_file).unwrap().exchanges_info;
    let mut symbols = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&args.symbols_file)
        .unwrap();
    let mut sym_count:HashMap<String, u32> = HashMap::new();
    let mut exchanges : HashMap<&str, Box<dyn Exchange>> = HashMap::new();
    let mut enabled_counter = 0u32;

    for ex_info in exchanges_info {

        if ex_info.is_enabled {
            enabled_counter += 1u32;
            info!("Pulling symbols from {}", &ex_info.name);
            let json = pull_symbols(&ex_info.symbols_url);
            let symbols = exchanges.entry(&ex_info.name)
            .or_insert(build_exchange(&ex_info.name))
            .parse_symbols(&json);

            for symbol in &symbols {
                sym_count
                    .entry(symbol.to_string())
                    .and_modify(|count| *count += 1u32)
                    .or_insert(1u32);
            }
        }
    }

    info!("Writing symbols");
    for (ref sym, count) in sym_count.into_iter() {
        if count == enabled_counter {
            if let Err(e) = writeln!(symbols, "{}", sym) {
                error!("Couldn't write to file: {}", e);
            }
        }
    }
}