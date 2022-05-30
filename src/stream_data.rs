use rock::core::{read_config, build_subscribe_msgs, load_symbols_from_file, OrderBook};
use clap::Parser;
use std::collections::HashMap;
#[macro_use]
extern crate log;

/// Program subscribes for market data accoridng to provided symbols and exchange config. Top 10 bids/asks and spreads are then streamed using grpc server
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Path to exchanges config file
    #[clap(short, long)]
    config_file: String,

    /// Path to avialable symbols file
    #[clap(short, long)]
    symbols_file: String,
}

fn main() {
    env_logger::init();
    
    let args = Args::parse();
    let exchanges = &read_config(&args.config_file).unwrap().exchanges;
    let symbols = load_symbols_from_file(&args.symbols_file);

    for ex in exchanges {
        if ex.is_enabled {
            info!("Subscribing for {}", &ex.name);
            let msgs = build_subscribe_msgs(&ex, &symbols);
            let book = OrderBook::new();

            for m in &msgs {
                info!("{}", m);
            }
            // call api, track resp, read subscrine resp, build book, register read handler
        }
    }
}

// create common mapping bitstamp format BTC/USD
// binance json base asset BTC / quote asset USD

// use tokio tungstenite for async wss client, runt on wss support
// use tokio grpc for grpc server
// use rust_decimal for prices and qty's
