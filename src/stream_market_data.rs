use rock::config::{read_config, build_subscribe_msgs, load_symbols};
use rock::order_book::OrderBook;
use clap::Parser;
use std::collections::HashMap;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, connect_async, tungstenite::protocol::Message};
use futures_util::{StreamExt, SinkExt};
use futures::future::join_all;
use tokio::net::TcpStream;
use url::Url;
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

async fn read_handle(mut ws: WebSocketStream<MaybeTlsStream<TcpStream>>) {
    env_logger::init();
    info!("Spawned");
    while let Some(msg) = ws.next().await {
        let msg = msg.unwrap();
        if msg.is_text() {
            info!("Got:\n{}", msg);
            //ws.send(msg).await.unwrap();
        }
    }
    info!("Nothing");
}

#[tokio::main]
async fn main() {
    //env_logger::init();
    
    let args = Args::parse();
    let exchanges = &read_config(&args.config_file).unwrap().exchanges;
    let symbols = load_symbols(&args.symbols_file);
    //let mut connections : HashMap<&Exchange, &mut WebSocketStream<MaybeTlsStream<TcpStream>>> = HashMap::new();
    let mut order_books : HashMap<&str, OrderBook> = HashMap::new();
    let mut handles = vec![];

    for ex in exchanges {
        if ex.is_enabled {
            //info!("Subscribing for {}", &ex.name);
            let msgs = build_subscribe_msgs(&ex, &symbols);
            let book = OrderBook::new();
            let (mut ws, _) = connect_async(
                Url::parse(&ex.api_url).expect(&format!("Can't connect to {}", &ex.api_url)),
            )
            .await.unwrap();
            
            for m in &msgs {
                //info!("{}", m);
                ws.send(Message::Text(m.to_string())).await.unwrap();
            }

            handles.push(tokio::spawn(async move{ read_handle(ws).await; }));

            //connections.insert(&ex, &mut ws);
            // call api, track resp, read subscrine resp, build book, register read handler
        }
    }
    futures::future::join_all(handles).await;
}

// create common mapping bitstamp format BTC/USD
// binance json base asset BTC / quote asset USD

// use tokio tungstenite for async wss client, runt on wss support
// use tokio grpc for grpc server
// use rust_decimal for prices and qty's
