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

    pub struct NoneExchange {
    }

    impl Exchange for NoneExchange {
        fn parse_symbols(&self, _: &Value) -> Vec<String> {
            vec![]
        }

        fn build_subscribe_msgs(&self, _: &Vec<String>) -> Vec<String> {
            vec![]
        }
    }

    pub fn build_exchange(exchange_name: &str) -> Box<dyn Exchange> {
        match exchange_name {
            "binance_com" => Box::new(BinanceCom{}),
            "bitstamp" => Box::new(Bitstamp{}),
            _ => Box::new(NoneExchange{})
        }
    }
}

pub mod config {

    use std::{
        fs::File,
        io::{prelude::*, BufReader},
        path::Path,
    };
    use crate::exchange::ExchangeInfo;
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
}

pub mod order_book {
    use std::cmp::{Ordering, Ord, PartialOrd, PartialEq, Eq};
    use rust_decimal::Decimal;
    use rust_decimal::prelude::*;
    use std::collections::BinaryHeap;

    const TOP_K: usize = 10;

    #[derive(Debug)]
    pub struct RawLevel<'a> {
        pub price: &'a str,
        pub qty: &'a str
    }

    #[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Clone, Hash, Copy)]
    pub struct Level<'a> {
        pub price: Decimal,
        pub qty: Decimal,
        pub exchange_name: &'a str,
    }

    #[derive(Debug, Eq, PartialEq, Ord, Clone, Hash, Copy)]
    pub struct BidLevel<'a> {
        pub data: Level<'a>
    }

    impl<'a> PartialOrd for BidLevel<'a> {
        #[inline]
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            (self.data.price, self.data.qty).partial_cmp(&(other.data.price, other.data.qty))
        }
    }

    #[derive(Debug, Eq, PartialEq, Ord, Clone, Hash, Copy)]
    pub struct AskLevel<'a> {
        pub data: Level<'a>
    }

    impl<'a> PartialOrd for AskLevel<'a> {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            match self.data.price.partial_cmp(&other.data.price) {
                Some(Ordering::Less) => Some(Ordering::Greater),
                Some(Ordering::Equal) => self.data.qty.partial_cmp(&other.data.qty),
                Some(Ordering::Greater) => Some(Ordering::Less),
                None => None
            }
        }
    }

    #[derive(Debug)]
    pub struct MergedOrderBook<'a> {
        // it's overkill for just 2 exchanges to use max/min heap for sorting levels
        // because algorithm for sorting of two sorted arrays could be use, however 
        // if more exchnges would be merged then max/min heap is optimal for sorting k sorted arrays
        bids: BinaryHeap<BidLevel<'a>>,
        asks: BinaryHeap<AskLevel<'a>>
    }

    impl<'a> MergedOrderBook<'a> {
        pub fn new() -> MergedOrderBook<'a> {
            MergedOrderBook{bids: BinaryHeap::new(), asks: BinaryHeap::new()}
        }

        pub fn clear(&mut self) {
            self.bids.clear();
            self.asks.clear();
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

        pub fn merge_snapshot(
                &mut self, 
                snapshot: &'a OrderBookSnapshot) {
            
            // this should be ensured at parser level
            assert!(snapshot.bids.len() <= TOP_K);
            assert!(snapshot.asks.len() <= TOP_K);

            for b in &snapshot.bids {
                self.bids.push(BidLevel{data: Level{price: Decimal::from_str(b.price).unwrap(), 
                                                          qty: Decimal::from_str(b.qty).unwrap(), 
                                                          exchange_name: snapshot.exchange_name}});
            }
            for a in &snapshot.asks {
                self.asks.push(AskLevel{data: Level{price: Decimal::from_str(a.price).unwrap(), 
                                                          qty: Decimal::from_str(a.qty).unwrap(), 
                                                          exchange_name: snapshot.exchange_name}});
            }
            self.keep_top_k(TOP_K);
        }

        pub fn drain_sorted_bids(&mut self) -> Vec<Level<'_>> {
            let mut result = vec![];
            result.reserve(self.bids.len());

            while !self.bids.is_empty() {
                result.push(self.bids.pop().unwrap().data);
            }
            result
        }

        pub fn drain_sorted_asks(&mut self) -> Vec<Level<'_>> {
            let mut result = vec![];
            result.reserve(self.asks.len());

            while !self.asks.is_empty() {
                result.push(self.asks.pop().unwrap().data);
            }
            result
        }
    }

    #[derive(Debug)]
    pub struct OrderBookSnapshot<'a> {
        // assumption: order book snapshots from exchagnes have data already sorted
        pub bids: Vec<RawLevel<'a>>,
        pub asks: Vec<RawLevel<'a>>,
        pub exchange_name: &'a str
    }

    impl<'a> OrderBookSnapshot<'a> {
        pub fn new() -> OrderBookSnapshot<'a> {
            OrderBookSnapshot{bids: vec![], asks: vec![], exchange_name: ""}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::order_book::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_merged_order_book_empty_snapshot_gives_nothing_merged() {
        let bitstamp = "bitstamp";
        let mut mob = MergedOrderBook::new();
        let snap1 = OrderBookSnapshot{bids: vec![], asks: vec![], exchange_name: bitstamp};
        mob.merge_snapshot(&snap1);

        assert_eq!(mob.drain_sorted_bids(), vec![]);
        assert_eq!(mob.drain_sorted_asks(), vec![]);
    }

    #[test]
    fn test_merged_order_book_single_snapshot_gives_itself_back() {
        let bitstamp = "bitstamp";
        let mut mob = MergedOrderBook::new();
        let snap1 = OrderBookSnapshot{
            bids: vec![RawLevel{price: "10", qty: "1"},
                       RawLevel{price: "9", qty: "1"},
                       RawLevel{price: "8", qty: "1"},
                       RawLevel{price: "7", qty: "1"},
                       RawLevel{price: "6", qty: "1"},
                       RawLevel{price: "5", qty: "1"},
                       RawLevel{price: "4", qty: "1"},
                       RawLevel{price: "3", qty: "1"},
                       RawLevel{price: "2", qty: "1"},
                       RawLevel{price: "1", qty: "1"}], 
            asks: vec![RawLevel{price: "11", qty: "1"},
                       RawLevel{price: "12", qty: "1"},
                       RawLevel{price: "13", qty: "1"},
                       RawLevel{price: "14", qty: "1"},
                       RawLevel{price: "15", qty: "1"},
                       RawLevel{price: "16", qty: "1"},
                       RawLevel{price: "17", qty: "1"},
                       RawLevel{price: "18", qty: "1"},
                       RawLevel{price: "19", qty: "1"},
                       RawLevel{price: "20", qty: "1"}], 
            exchange_name: bitstamp};
        mob.merge_snapshot(&snap1);   

        assert_eq!(mob.drain_sorted_bids(), vec![
            Level{price: dec!(10), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(9), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(8), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(7), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(6), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(5), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(4), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(3), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(2), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(1), qty: dec!(1), exchange_name: bitstamp}]);
        assert_eq!(mob.drain_sorted_asks(), vec![
            Level{price: dec!(11), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(12), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(13), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(14), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(15), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(16), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(17), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(18), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(19), qty: dec!(1), exchange_name: bitstamp},
            Level{price: dec!(20), qty: dec!(1), exchange_name: bitstamp}]);
    }

    #[test]
    fn test_merged_order_book_multiple_snapshots_merged_sorted_and_capped() {
        let bitstamp = "bitstamp";
        let binance_com = "binance_com";
        let mut mob = MergedOrderBook::new();
        let snap1 = OrderBookSnapshot{
            bids: vec![RawLevel{price: "10", qty: "1"},
                       RawLevel{price: "9", qty: "1"},
                       RawLevel{price: "8", qty: "1"},
                       RawLevel{price: "7", qty: "1"},
                       RawLevel{price: "6", qty: "1"},
                       RawLevel{price: "5", qty: "1"},
                       RawLevel{price: "4", qty: "1"},
                       RawLevel{price: "3", qty: "1"},
                       RawLevel{price: "2", qty: "1"},
                       RawLevel{price: "1", qty: "1"}], 
            asks: vec![RawLevel{price: "11", qty: "1"},
                       RawLevel{price: "12", qty: "1"},
                       RawLevel{price: "13", qty: "1"},
                       RawLevel{price: "14", qty: "1"},
                       RawLevel{price: "15", qty: "1"},
                       RawLevel{price: "16", qty: "1"},
                       RawLevel{price: "17", qty: "1"},
                       RawLevel{price: "18", qty: "1"},
                       RawLevel{price: "19", qty: "1"},
                       RawLevel{price: "20", qty: "1"}], 
            exchange_name: bitstamp};

        mob.merge_snapshot(&snap1);   

        let snap2 = OrderBookSnapshot{
            bids: vec![RawLevel{price: "10", qty: "2"},
                        RawLevel{price: "9", qty: "2"},
                        RawLevel{price: "8", qty: "2"},
                        RawLevel{price: "7", qty: "2"},
                        RawLevel{price: "6", qty: "1"},
                        RawLevel{price: "5", qty: "2"},
                        RawLevel{price: "4", qty: "2"},
                        RawLevel{price: "3", qty: "2"},
                        RawLevel{price: "2", qty: "2"},
                        RawLevel{price: "1", qty: "2"}], 
            asks: vec![RawLevel{price: "11", qty: "2"},
                        RawLevel{price: "12", qty: "2"},
                        RawLevel{price: "13", qty: "2"},
                        RawLevel{price: "14", qty: "2"},
                        RawLevel{price: "15", qty: "2"},
                        RawLevel{price: "16", qty: "2"},
                        RawLevel{price: "17", qty: "2"},
                        RawLevel{price: "18", qty: "2"},
                        RawLevel{price: "19", qty: "2"},
                        RawLevel{price: "20", qty: "2"}], 
            exchange_name: binance_com};

            mob.merge_snapshot(&snap2);

            // following checks are made here:
            // - for bids the price level with biggest price is on top
            // - for asks the price level with lowest price is on top
            // - for price levels with same price the price level with biggest qty is closer to top
            // - for price levels with same price and qty the price level that was added first is closer to top
            assert_eq!(mob.drain_sorted_bids(), vec![
                Level{price: dec!(10), qty: dec!(2), exchange_name: binance_com},
                Level{price: dec!(10), qty: dec!(1), exchange_name: bitstamp},
                Level{price: dec!(9), qty: dec!(2), exchange_name: binance_com},
                Level{price: dec!(9), qty: dec!(1), exchange_name: bitstamp},
                Level{price: dec!(8), qty: dec!(2), exchange_name: binance_com},
                Level{price: dec!(8), qty: dec!(1), exchange_name: bitstamp},
                Level{price: dec!(7), qty: dec!(2), exchange_name: binance_com},
                Level{price: dec!(7), qty: dec!(1), exchange_name: bitstamp},
                Level{price: dec!(6), qty: dec!(1), exchange_name: bitstamp},
                Level{price: dec!(6), qty: dec!(1), exchange_name: binance_com}]);
            assert_eq!(mob.drain_sorted_asks(), vec![
                Level{price: dec!(11), qty: dec!(2), exchange_name: binance_com},
                Level{price: dec!(11), qty: dec!(1), exchange_name: bitstamp},
                Level{price: dec!(12), qty: dec!(2), exchange_name: binance_com},
                Level{price: dec!(12), qty: dec!(1), exchange_name: bitstamp},
                Level{price: dec!(13), qty: dec!(2), exchange_name: binance_com},
                Level{price: dec!(13), qty: dec!(1), exchange_name: bitstamp},
                Level{price: dec!(14), qty: dec!(2), exchange_name: binance_com},
                Level{price: dec!(14), qty: dec!(1), exchange_name: bitstamp},
                Level{price: dec!(15), qty: dec!(2), exchange_name: binance_com},
                Level{price: dec!(15), qty: dec!(1), exchange_name: bitstamp}]);

            assert_eq!(mob.drain_sorted_bids(), vec![]);
            assert_eq!(mob.drain_sorted_asks(), vec![]);
    }
}
