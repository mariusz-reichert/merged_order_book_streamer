use rock::config::{read_config, load_symbols};
use rock::order_book::{MergedOrderBook, Level as BookLevel};
use rock::exchange::{Exchange, build_exchange};
use clap::Parser;
use std::collections::HashMap;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, connect_async, tungstenite::protocol::Message};
use futures_util::{StreamExt, SinkExt};
use tokio::net::TcpStream;
use url::Url;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tonic::{transport::Server, Request, Response, Status};
use orderbook::orderbook_aggregator_server::{OrderbookAggregator, OrderbookAggregatorServer};
use orderbook::{Empty, Summary, Level};
use tokio_stream::wrappers::ReceiverStream;
use tokio::sync::mpsc;
use rust_decimal::prelude::*;

mod orderbook {
    include!("orderbook.rs");
}

#[macro_use]
extern crate log;

type MergedOrderBooks = Arc<Mutex<HashMap<String, MergedOrderBook>>>;

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

fn print_summary_to_log(symbol: &str, bids: &Vec<BookLevel>, asks: &Vec<BookLevel>) {
    info!("Merged order book for {}", symbol);
    info!("top 10 bids:");
    for (i, b) in bids.iter().enumerate() {
        info!("{}: px: {}, qty: {}", i, b.price, b.qty);
    }
    info!("top 10 asks:");
    for (i, a) in asks.iter().enumerate() {
        info!("{}: px: {}, qty: {}", i, a.price, a.qty);
    }
    if bids.len() > 0 && asks.len() > 0 { 
         
        info!("spread: {}\n", asks[0].price - bids[0].price );
    } else { 
        info!("spread: None\n"); 
    }
}

#[derive(Default)]
pub struct OrderbookAggregatorImpl {
    mobs: MergedOrderBooks
}

#[tonic::async_trait]
impl OrderbookAggregator for OrderbookAggregatorImpl {

    type BookSummaryStream = ReceiverStream<Result<Summary, Status>>;

    async fn book_summary(
        &self,
        request: Request<Empty>) -> Result<Response<Self::BookSummaryStream>, Status> {
        info!("Request from {:?}", request.remote_addr());

        let (tx, rx) = mpsc::channel(4);
        let mobs = self.mobs.lock().unwrap().clone();
        tokio::spawn(async move {
            
            for (_symbol, mob) in mobs.iter() {
                let mut mob_to_publish = mob.clone();
                let top_10_bids = mob_to_publish.drain_sorted_bids();
                let top_10_asks= mob_to_publish.drain_sorted_asks();
                let mut _spread = 0.0;
                
                if top_10_bids.len() > 0 && top_10_asks.len() > 0 { 
                    // todo: handle possible loss of precision
                    _spread = (top_10_asks[0].price - top_10_bids[0].price).to_f64().unwrap();
                }

                tx.send(Ok(Summary {
                    symbol: _symbol.clone(),
                    spread:_spread,
                    bids: top_10_bids.iter().map(
                        |l| Level{ 
                            exchange: l.exchange_name.clone(), 
                            price: l.price.to_f64().unwrap(), 
                            amount: l.qty.to_f64().unwrap()}).collect(),
                    asks: top_10_asks.iter().map(
                        |l| Level{ 
                            exchange: l.exchange_name.clone(), 
                            price: l.price.to_f64().unwrap(), 
                            amount: l.qty.to_f64().unwrap()}).collect()
                })).await.unwrap();
            }
        });
    
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

async fn ws_reader(
    mut ws: WebSocketStream<MaybeTlsStream<TcpStream>>, 
    exchange: Arc<dyn Exchange + Send + Sync>,
    mobs: MergedOrderBooks) 
{
    while let Some(msg) = ws.next().await {
        let msg = msg.unwrap();
        let msg = match msg {
            Message::Text(s) => s,
            _ => { info!("Text not found in msg."); continue; }
        };

        let value: Value = serde_json::from_str(&msg).expect("Unable to parse message");
        match exchange.parse_snapshot(&value) {
            Some(snap) => {
                let mut mobs = mobs.lock().unwrap();
                let mob = mobs.entry(snap.symbol.clone()).or_insert(MergedOrderBook::new());
                mob.merge_snapshot(snap.clone());

                
                let mut mob_to_publish = mob.clone();
                let top_10_bids = mob_to_publish.drain_sorted_bids();
                let top_10_asks= mob_to_publish.drain_sorted_asks();
                print_summary_to_log(snap.symbol.as_str(), &top_10_bids, &top_10_asks);
                

           },
           None => ()
        }
        
        // todo: implement grpc server
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    
    let args = Args::parse();
    let exchanges_info = &read_config(&args.config_file).unwrap().exchanges_info;
    let symbols = load_symbols(&args.symbols_file);

    let mobs: MergedOrderBooks = Arc::new(Mutex::new(HashMap::<String, MergedOrderBook>::new()));
    let mut handles = vec![];
    let mut exchanges : HashMap<&str, Arc<dyn Exchange + Send + Sync>> = HashMap::new();

    // todo: properly handle option values instead of unwrap
    for ex_info in exchanges_info {
        if ex_info.is_enabled {
            
            let subscribe_msgs = exchanges.entry(&ex_info.name.as_str())
                                                       .or_insert(build_exchange(ex_info.name.clone()).unwrap())
                                                       .build_subscribe_msgs(&symbols);

            if !subscribe_msgs.is_empty() {
        
                let (mut ws, _) = connect_async(
                    Url::parse(&ex_info.api_url).expect(&format!("Can't connect to {}", &ex_info.api_url)),
                )
                .await.unwrap();
                
                for sub in &subscribe_msgs {
                    ws.send(Message::Text(sub.to_string())).await.unwrap();
                    // todo: handle failed subscriptions
                }
            
                let ex = build_exchange(ex_info.name.clone()).unwrap();
                let mobs = mobs.clone();
                handles.push(tokio::spawn(async move{ ws_reader(ws, ex, mobs).await; }));

            }
        }
    }

    let addr = "[::1]:50051".parse().unwrap();
    let mut orderbook_aggregator = OrderbookAggregatorImpl::default();
    orderbook_aggregator.mobs = mobs;

    info!("Order book aggregator server listening on {}", addr);

    Server::builder()
        .accept_http1(true)
        .add_service(OrderbookAggregatorServer::new(orderbook_aggregator))
        .serve(addr)
        .await.unwrap();

    for h in handles {
        h.await.unwrap();
    }
}
