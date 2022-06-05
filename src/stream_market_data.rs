use rock::config::{read_config, load_symbols};
use rock::order_book::{MergedOrderBook, OrderBookSnapshot};
use rock::exchange::{Exchange, build_exchange};
use clap::Parser;
use std::collections::HashMap;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, connect_async, tungstenite::protocol::Message};
use futures_util::{StreamExt, SinkExt};
use tokio::net::TcpStream;
use url::Url;
use serde_json::Value;
use std::sync::{Arc, Mutex};
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

async fn ws_reader(mut ws: WebSocketStream<MaybeTlsStream<TcpStream>>, exchange: Arc<dyn Exchange + Send + Sync>) 
{
    info!("Spawned");
    while let Some(msg) = ws.next().await {
        let msg = msg.unwrap();
        if msg.is_text() {
            
            let json: Value = serde_json::from_str(&msg.to_text().unwrap()).unwrap();
            let p = exchange.parse_snapshot(&json);
            match p {
                Some(v) => info!("Snap:\n{:?}", v),
                None => ()
            }
            
            // update proper snapshot
            // calculate merged orderbook
            // propagate to grpc server

        }
    }
    info!("Nothing");
}

#[tokio::main]
async fn main() {
    env_logger::init();
    
    let args = Args::parse();
    let exchanges_info = &read_config(&args.config_file).unwrap().exchanges_info;
    let symbols = load_symbols(&args.symbols_file);

    //let mut connections : HashMap<&str, &mut WebSocketStream<MaybeTlsStream<TcpStream>>> = HashMap::new();
    let mut snapshots : HashMap<&str, OrderBookSnapshot> = HashMap::new();
    let mut handles = vec![];
    let mut exchanges : HashMap<&str, Arc<dyn Exchange + Send + Sync>> = HashMap::new();

    // todo: properly handle option values instead of unwrap
    for ex_info in exchanges_info {
        if ex_info.is_enabled {
            //info!("Subscribing for {}", &ex.name);
            let subscribe_msgs = exchanges.entry(&ex_info.name.as_str())
                                                       .or_insert(build_exchange(&ex_info.name.as_str()).unwrap())
                                                       .build_subscribe_msgs(&symbols);

            if !subscribe_msgs.is_empty() {
        
                let (mut ws, _) = connect_async(
                    Url::parse(&ex_info.api_url).expect(&format!("Can't connect to {}", &ex_info.api_url)),
                )
                .await.unwrap();
                
                for sub in &subscribe_msgs {
                    //info!("{}", m);
                    ws.send(Message::Text(sub.to_string())).await.unwrap();
                    // todo: handle failed subscriptions
                }
            
                let ex = build_exchange(&ex_info.name.as_str()).unwrap();
                handles.push(tokio::spawn(async move{ ws_reader(ws, ex).await; }));

            }

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
