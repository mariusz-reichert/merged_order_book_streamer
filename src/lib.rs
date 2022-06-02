pub mod exchange {
    use std::cmp::{PartialEq, Eq};
    use std::hash::{Hash, Hasher};
    use serde_json::Value;
    use serde::{Deserialize, Serialize};

    pub trait Exchange {
        fn parse_symbols(&self, json_source: &Value) -> Vec<String>;
        fn build_subscribe_msgs(&self, symbols: &Vec<String>) -> Vec<String>;
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct ExchangeInfo {
        pub name: String,
        pub is_enabled: bool,
        pub symbols_url:String,
        pub api_url: String
    }

    impl Hash for ExchangeInfo {
        #[inline]
        fn hash<H: Hasher>(&self, hasher: &mut H) {
            self.name.hash(hasher);
        }
    }

    impl PartialEq for ExchangeInfo {
        #[inline]
        fn eq(&self, other: &Self) -> bool {
            self.name == other.name
        }
    }

    impl Eq for ExchangeInfo {}

    pub struct BinanceCom {
    }

    impl Exchange for BinanceCom {
        fn parse_symbols(&self, json_source: &Value) -> Vec<String> {
            let mut symbols = vec![];
            for s in json_source.get("symbols").unwrap().as_array().unwrap() {
                symbols.push(format!("{}{}", s.get("baseAsset").unwrap().as_str().unwrap().to_lowercase(), 
                                             s.get("quoteAsset").unwrap().as_str().unwrap().to_lowercase()));
            }
            symbols
        }
        fn build_subscribe_msgs(&self, symbols: &Vec<String>) -> Vec<String> {
            let mut subscribe_msgs = vec![];
            let mut channels = String::new();
            for s in symbols {
                channels.push_str(format!("\"{}@depth20@100ms\",", s).as_str());
            }
            // remove trailing ','
            channels.pop();
            subscribe_msgs.push(format!("{{\"method\":\"SUBSCRIBE\",\"params\":[{}],\"id\":1}}", channels));
            subscribe_msgs
        }
    }

    pub struct Bitstamp {
    }

    impl Exchange for Bitstamp {
        fn parse_symbols(&self, json_source: &Value) -> Vec<String> {
            let mut symbols = vec![];
            for s in json_source.as_array().unwrap() {
                symbols.push(s.get("url_symbol").unwrap().as_str().unwrap().to_string());
            }
            symbols
        }
        fn build_subscribe_msgs(&self, symbols: &Vec<String>) -> Vec<String> {
            let mut subscribe_msgs = vec![];
            for s in symbols {
                subscribe_msgs.push(format!("{{\"event\":\"bts:subscribe\",\"data\":{{\"channel\":\"order_book_{}\"}}}}", s));
            }
            subscribe_msgs
        }
    }
}

pub mod order_book {
    use std::cmp::{Ordering, Ord, PartialOrd, PartialEq, Eq};
    use rust_decimal::Decimal;
    use std::collections::BinaryHeap;

    const TOP_K: usize = 10;

    #[derive(Eq, PartialEq, PartialOrd, Ord, Clone)]
    pub struct PriceLevel<'a> {
        pub price: Decimal,
        pub qty: Decimal,
        pub exchange: &'a str
    }

    #[derive(Eq, PartialEq, PartialOrd)]
    pub struct BidPriceLevel<'a> {
        pub data: PriceLevel<'a>
    }

    impl<'a> Ord for BidPriceLevel<'a> {
        #[inline]
        fn cmp(&self, other: &Self) -> Ordering {
            (self.data.price, self.data.qty).cmp(&(other.data.price, other.data.qty))
        }
    }

    #[derive(Eq, PartialEq, PartialOrd)]
    pub struct AskPriceLevel<'a> {
        pub data: PriceLevel<'a>
    }

    impl<'a> Ord for AskPriceLevel<'a> {
        #[inline]
        fn cmp(&self, other: &Self) -> Ordering {
            match self.data.price.cmp(&other.data.price) {
                Ordering::Less => Ordering::Greater,
                Ordering::Equal => self.data.qty.cmp(&other.data.qty),
                Ordering::Greater => Ordering::Less
            }
        }
    }

    pub struct OrderBook<'a> {
        pub bids: BinaryHeap<BidPriceLevel<'a>>,
        pub asks: BinaryHeap<AskPriceLevel<'a>>
    }

    impl<'a> OrderBook<'a> {
        pub fn new() -> OrderBook<'a> {
            OrderBook{bids: BinaryHeap::new(), asks: BinaryHeap::new()}
        }

        fn keep_top_k(&mut self, k: usize) {
            
            if self.bids.len() > k {
                let mut tmp = BinaryHeap::new();
                while tmp.len() < k {
                    tmp.push(self.bids.pop().unwrap());
                }
                self.bids = tmp;
            }

            if self.asks.len() > k {
                let mut tmp = BinaryHeap::new();
                while tmp.len() < k {
                    tmp.push(self.asks.pop().unwrap());
                }
                self.asks = tmp;
            }
        }

        pub fn add_price_levels(
                &mut self, 
                new_bids: &Vec<PriceLevel<'a>>,
                new_asks: &Vec<PriceLevel<'a>>) {
            for b in new_bids {
                self.bids.push(BidPriceLevel{data: b.clone()});
            }
            for a in new_asks {
                self.asks.push(AskPriceLevel{data: a.clone()});
            }
            self.keep_top_k(TOP_K);
        }
    }
}

pub mod config {

    use std::{
        fs::File,
        io::{prelude::*, BufReader},
        path::Path,
    };
    use crate::exchange::*;
    use serde_json::Value;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, Serialize)]
    pub struct Config {
        pub exchanges_info: Vec<ExchangeInfo>
    }

    pub fn read_config(path: &str) -> std::io::Result<Config> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn pull_symbols(url: &str) -> Value {
        let response = reqwest::blocking::get(url).unwrap().text().unwrap();
        serde_json::from_str(&response).unwrap()
    }

    pub fn load_symbols(filename: impl AsRef<Path>) -> Vec<String> {
        let file = File::open(filename).expect("No such file");
        let buf = BufReader::new(file);
        buf.lines()
            .map(|l| l.expect("Could not parse line"))
            .collect()
    }

    pub fn parse_symbols(exchange_name: &str, json: &Value) -> Vec<String> {
        match exchange_name {
            "binance_com" => BinanceCom{}.parse_symbols(&json),
            "bitstamp" => Bitstamp{}.parse_symbols(&json),
            _ => Vec::<String>::new()
        }
    }

    pub fn build_subscribe_msgs(exchange_name: &str, symbols: &Vec<String>) -> Vec<String> {
        match exchange_name {
            "binance_com" => BinanceCom{}.build_subscribe_msgs(&symbols),
            "bitstamp" => Bitstamp{}.build_subscribe_msgs(&symbols),
            _ => Vec::<String>::new()
        }
    }
}

