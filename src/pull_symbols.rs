use serde_json::Value;
use rock::core::{read_config, get_json_symbols};
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
    let exchanges = &read_config(&args.config_file).unwrap().exchanges;
    let mut symbols = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&args.symbols_file)
        .unwrap();
    let mut sym_count:HashMap<String, u32> = HashMap::new();
    let mut enabled_exchanges = 0u32;

    for ex in exchanges {
        if ex.is_enabled {
            enabled_exchanges += 1u32;
            info!("Pulling symbols from {}", &ex.name);
            let response = reqwest::blocking::get(&ex.symbols_url).unwrap().text().unwrap();
            let json: Value = serde_json::from_str(&response).unwrap();
            let symbols = get_json_symbols(&ex.name, &json);
    
            for s in symbols {
                sym_count
                    .entry(s)
                    .and_modify(|c| *c += 1u32)
                    .or_insert(1u32);
            }
        }
    }

    info!("Writing symbols");
    for (ref sym, count) in sym_count.into_iter() {
        if count == enabled_exchanges {
            if let Err(e) = writeln!(symbols, "{}", sym) {
                error!("Couldn't write to file: {}", e);
            }
        }
    }
}