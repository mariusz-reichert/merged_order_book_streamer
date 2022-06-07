pub mod exchange {
    use std::cmp::{PartialEq, Eq};
    use std::hash::{Hash, Hasher};
    use serde_json::Value;
    use serde::{Deserialize, Serialize};
    use crate::order_book::OrderBookSnapshot;
    use std::sync::Arc;
    use rust_decimal::Decimal;
    use rust_decimal::prelude::*;
    use crate::order_book::TOP_K;

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

    pub trait Exchange {
        fn parse_symbols(&self, json: &Value) -> Vec<String>;
        fn build_subscribe_msgs(&self, symbols: &Vec<String>) -> Vec<String>;
        fn parse_snapshot(&self, json: &Value) -> Option<OrderBookSnapshot>;
    }

    pub struct BinanceCom;

    impl Exchange for BinanceCom {
        fn parse_symbols(&self, json: &Value) -> Vec<String> {
            let mut symbols = vec![];
            for s in json.get("symbols").unwrap().as_array().unwrap() {
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

        fn parse_snapshot(&self, json: &Value) -> Option<OrderBookSnapshot> {
            match json.get("stream") {
                Some(v) => {
                    let stream = v.as_str().unwrap();
                    let delim_idx = stream.find("@").unwrap_or(0usize);
                    let s: &str = &stream[0..delim_idx];

                    // todo: find a way to truncate json vector instead of parsed vector
                    let json_bids = json.get("data").unwrap().get("bids").unwrap().as_array().unwrap();
                    let json_asks = json.get("data").unwrap().get("asks").unwrap().as_array().unwrap();

                    let mut raw_bids = json_bids.into_iter().map(|e| {
                        [ Decimal::from_str(e.as_array().unwrap()[0].as_str().unwrap()).unwrap(), 
                          Decimal::from_str(e.as_array().unwrap()[1].as_str().unwrap()).unwrap()]
                    }).collect::<Vec<_>>();
                    raw_bids.truncate(TOP_K);
                    let mut raw_asks = json_asks.into_iter().map(|e| {
                        [ Decimal::from_str(e.as_array().unwrap()[0].as_str().unwrap()).unwrap(), 
                          Decimal::from_str(e.as_array().unwrap()[1].as_str().unwrap()).unwrap()]
                    }).collect::<Vec<_>>();
                    raw_asks.truncate(TOP_K);

                    Some(OrderBookSnapshot{
                        bids: raw_bids, 
                        asks: raw_asks, 
                        exchange_name: "binance_com".to_string(), 
                        symbol: s.to_string()})
                },
                None => None
            }
        }

    }

    pub struct Bitstamp;

    impl Exchange for Bitstamp {
        fn parse_symbols(&self, json: &Value) -> Vec<String> {
            let mut symbols = vec![];
            for s in json.as_array().unwrap() {
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

        fn parse_snapshot(&self, json: &Value) -> Option<OrderBookSnapshot> {
            
            let c  = json.get("channel");
            match c {
                Some(c) => {

                    if json.get("event").unwrap().as_str().unwrap() != "data" {
                        return None;
                    }
                    
                    let channel = c.as_str().unwrap();
                    let delim_idx = channel.rfind("_").unwrap_or(0usize);
                    let s: &str = &channel[delim_idx+1..channel.len()];

                    // todo: find a way to truncate json vector instead of parsed vector
                    let json_bids = json.get("data").unwrap().get("bids").unwrap().as_array().unwrap();
                    let json_asks = json.get("data").unwrap().get("asks").unwrap().as_array().unwrap();

                    let mut raw_bids = json_bids.into_iter().map(|e| {
                        [ Decimal::from_str(e.as_array().unwrap()[0].as_str().unwrap()).unwrap(), 
                          Decimal::from_str(e.as_array().unwrap()[1].as_str().unwrap()).unwrap()]
                    }).collect::<Vec<_>>();
                    raw_bids.truncate(TOP_K);
                    let mut raw_asks = json_asks.into_iter().map(|e| {
                        [ Decimal::from_str(e.as_array().unwrap()[0].as_str().unwrap()).unwrap(), 
                          Decimal::from_str(e.as_array().unwrap()[1].as_str().unwrap()).unwrap()]
                    }).collect::<Vec<_>>();
                    raw_asks.truncate(TOP_K);

                    Some(OrderBookSnapshot{
                        bids: raw_bids, 
                        asks: raw_asks, 
                        exchange_name: "bitstamp".to_string(), 
                        symbol: s.to_string()})
                },
                None => None
            }
        }
    }

    pub fn build_exchange(exchange_name: String) -> Option<Arc<dyn Exchange + Send + Sync>> {
        match exchange_name.as_str() {
            "binance_com" => Some(Arc::new(BinanceCom{})),
            "bitstamp" => Some(Arc::new(Bitstamp{})),
            _ => None
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
    use std::collections::BinaryHeap;

    pub const TOP_K: usize = 10;

    #[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Clone, Hash)]
    pub struct Level {
        pub price: Decimal,
        pub qty: Decimal,
        pub exchange_name: String,
    }

    #[derive(Debug, Eq, PartialEq, Ord, Clone, Hash)]
    pub struct BidLevel {
        pub data: Level
    }

    impl PartialOrd for BidLevel {
        #[inline]
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            (self.data.price, self.data.qty).partial_cmp(&(other.data.price, other.data.qty))
        }
    }

    #[derive(Debug, Eq, PartialEq, Ord, Clone, Hash)]
    pub struct AskLevel {
        pub data: Level
    }

    impl PartialOrd for AskLevel {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            match self.data.price.partial_cmp(&other.data.price) {
                Some(Ordering::Less) => Some(Ordering::Greater),
                Some(Ordering::Equal) => self.data.qty.partial_cmp(&other.data.qty),
                Some(Ordering::Greater) => Some(Ordering::Less),
                None => None
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct MergedOrderBook {
        // it's overkill for just 2 exchanges to use max/min heap for sorting levels
        // because algorithm for sorting of two sorted arrays could be use, however 
        // if more exchnges would be merged then max/min heap is optimal for sorting k sorted arrays
        bids: BinaryHeap<BidLevel>,
        asks: BinaryHeap<AskLevel>
    }

    impl MergedOrderBook {
        pub fn new() -> MergedOrderBook {
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

        fn remove_old_entries(&mut self, exchange_name: String) {
            let mut tmp = BinaryHeap::new();
            while !self.bids.is_empty() {
                let level = self.bids.pop().unwrap();
                if level.data.exchange_name != exchange_name {
                    tmp.push(level);
                }
            }
            self.bids = tmp;

            let mut tmp = BinaryHeap::new();
            while !self.asks.is_empty() {
                let level = self.asks.pop().unwrap();
                if level.data.exchange_name != exchange_name {
                    tmp.push(level);
                }
            }
            self.asks = tmp;
        }

        // todo: find a way of passing OrderBookSnapshot as a ref
        pub fn merge_snapshot(
                &mut self, 
                snapshot: OrderBookSnapshot) {
            
            // this should be ensured at parser level
            assert!(snapshot.bids.len() <= TOP_K);
            assert!(snapshot.asks.len() <= TOP_K);

            self.remove_old_entries(snapshot.exchange_name.to_string());

            for b in &snapshot.bids {
                self.bids.push(BidLevel{data: Level{price: b[0], 
                                                          qty: b[1], 
                                                          exchange_name: snapshot.exchange_name.to_string()}});
            }
            for a in &snapshot.asks {
                self.asks.push(AskLevel{data: Level{price: a[0], 
                                                          qty: a[1], 
                                                          exchange_name: snapshot.exchange_name.to_string()}});
            }
            self.keep_top_k(TOP_K);
        }

        pub fn drain_sorted_bids(&mut self) -> Vec<Level> {
            let mut result = vec![];
            result.reserve(self.bids.len());

            while !self.bids.is_empty() {
                result.push(self.bids.pop().unwrap().data);
            }
            result
        }

        pub fn drain_sorted_asks(&mut self) -> Vec<Level> {
            let mut result = vec![];
            result.reserve(self.asks.len());

            while !self.asks.is_empty() {
                result.push(self.asks.pop().unwrap().data);
            }
            result
        }
    }

    type RawLevel = [Decimal; 2usize];

    #[derive(Debug, Hash, Eq, PartialEq, Clone)]
    pub struct OrderBookSnapshot {
        // assumption: order book snapshots from exchagnes has data that is already sorted
        pub bids: Vec<RawLevel>,
        pub asks: Vec<RawLevel>,
        pub exchange_name: String,
        pub symbol: String
    }

    impl OrderBookSnapshot {
        pub fn new() -> OrderBookSnapshot {
            OrderBookSnapshot{bids: vec![], asks: vec![], exchange_name: "".to_string(), symbol: "".to_string()}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    use super::order_book::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_merged_order_book_empty_snapshot_gives_nothing_merged() {
        let bitstamp = "bitstamp";
        let mut mob = MergedOrderBook::new();
        let snap1 = OrderBookSnapshot{bids: vec![], asks: vec![], exchange_name: bitstamp.to_string(), symbol: "btcusd".to_string()};
        mob.merge_snapshot(snap1);

        assert_eq!(mob.drain_sorted_bids(), vec![]);
        assert_eq!(mob.drain_sorted_asks(), vec![]);
    }

    #[test]
    fn test_merged_order_book_single_snapshot_gives_itself_back() {
        let bitstamp = "bitstamp";
        let mut mob = MergedOrderBook::new();
        let snap1 = OrderBookSnapshot{
            bids: vec![[dec!(10), dec!(1)],
                       [dec!(9), dec!(1)],
                       [dec!(8), dec!(1)],
                       [dec!(7), dec!(1)],
                       [dec!(6), dec!(1)],
                       [dec!(5), dec!(1)],
                       [dec!(4), dec!(1)],
                       [dec!(3), dec!(1)],
                       [dec!(2), dec!(1)],
                       [dec!(1), dec!(1)]],
            asks: vec![[dec!(11), dec!(1)],
                       [dec!(12), dec!(1)],
                       [dec!(13), dec!(1)],
                       [dec!(14), dec!(1)],
                       [dec!(15), dec!(1)],
                       [dec!(16), dec!(1)],
                       [dec!(17), dec!(1)],
                       [dec!(18), dec!(1)],
                       [dec!(19), dec!(1)],
                       [dec!(20), dec!(1)]], 
            exchange_name: bitstamp.to_string(),
            symbol: "btcusd".to_string()};
        mob.merge_snapshot(snap1);   

        assert_eq!(mob.drain_sorted_bids(), vec![
            Level{price: dec!(10), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(9), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(8), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(7), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(6), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(5), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(4), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(3), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(2), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(1), qty: dec!(1), exchange_name: bitstamp.to_string()}]);
        assert_eq!(mob.drain_sorted_asks(), vec![
            Level{price: dec!(11), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(12), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(13), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(14), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(15), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(16), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(17), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(18), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(19), qty: dec!(1), exchange_name: bitstamp.to_string()},
            Level{price: dec!(20), qty: dec!(1), exchange_name: bitstamp.to_string()}]);
    }

    #[test]
    fn test_merged_order_book_multiple_snapshots_merged_sorted_and_capped() {
        let bitstamp = "bitstamp";
        let binance_com = "binance_com";
        let mut mob = MergedOrderBook::new();
        let snap1 = OrderBookSnapshot{
            bids: vec![[dec!(10), dec!(1)],
                       [dec!(9), dec!(1)],
                       [dec!(8), dec!(1)],
                       [dec!(7), dec!(1)],
                       [dec!(6), dec!(1)],
                       [dec!(5), dec!(1)],
                       [dec!(4), dec!(1)],
                       [dec!(3), dec!(1)],
                       [dec!(2), dec!(1)],
                       [dec!(1), dec!(1)]], 
            asks: vec![[dec!(11), dec!(1)],
                       [dec!(12), dec!(1)],
                       [dec!(13), dec!(1)],
                       [dec!(14), dec!(1)],
                       [dec!(15), dec!(1)],
                       [dec!(16), dec!(1)],
                       [dec!(17), dec!(1)],
                       [dec!(18), dec!(1)],
                       [dec!(19), dec!(1)],
                       [dec!(20), dec!(1)]], 
            exchange_name: bitstamp.to_string(),
            symbol: "btcusd".to_string()};

        mob.merge_snapshot(snap1);   

        let snap2 = OrderBookSnapshot{
            bids: vec![[dec!(10), dec!(2)],
                        [dec!(9), dec!(2)],
                        [dec!(8), dec!(2)],
                        [dec!(7), dec!(2)],
                        [dec!(6), dec!(1)],
                        [dec!(5), dec!(2)],
                        [dec!(4), dec!(2)],
                        [dec!(3), dec!(2)],
                        [dec!(2), dec!(2)],
                        [dec!(1), dec!(2)]], 
            asks: vec![[dec!(11), dec!(2)],
                        [dec!(12), dec!(2)],
                        [dec!(13), dec!(2)],
                        [dec!(14), dec!(2)],
                        [dec!(15), dec!(2)],
                        [dec!(16), dec!(2)],
                        [dec!(17), dec!(2)],
                        [dec!(18), dec!(2)],
                        [dec!(19), dec!(2)],
                        [dec!(20), dec!(2)]], 
            exchange_name: binance_com.to_string(),
            symbol: "btcusd".to_string()};

            mob.merge_snapshot(snap2);

            // following checks are made here:
            // - for bids the price level with biggest price is on top
            // - for asks the price level with lowest price is on top
            // - for price levels with same price the price level with biggest qty is closer to top
            // - for price levels with same price and qty the price level that was added first is closer to top
            assert_eq!(mob.drain_sorted_bids(), vec![
                Level{price: dec!(10), qty: dec!(2), exchange_name: binance_com.to_string()},
                Level{price: dec!(10), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(9), qty: dec!(2), exchange_name: binance_com.to_string()},
                Level{price: dec!(9), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(8), qty: dec!(2), exchange_name: binance_com.to_string()},
                Level{price: dec!(8), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(7), qty: dec!(2), exchange_name: binance_com.to_string()},
                Level{price: dec!(7), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(6), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(6), qty: dec!(1), exchange_name: binance_com.to_string()}]);
            assert_eq!(mob.drain_sorted_asks(), vec![
                Level{price: dec!(11), qty: dec!(2), exchange_name: binance_com.to_string()},
                Level{price: dec!(11), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(12), qty: dec!(2), exchange_name: binance_com.to_string()},
                Level{price: dec!(12), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(13), qty: dec!(2), exchange_name: binance_com.to_string()},
                Level{price: dec!(13), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(14), qty: dec!(2), exchange_name: binance_com.to_string()},
                Level{price: dec!(14), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(15), qty: dec!(2), exchange_name: binance_com.to_string()},
                Level{price: dec!(15), qty: dec!(1), exchange_name: bitstamp.to_string()}]);

            assert_eq!(mob.drain_sorted_bids(), vec![]);
            assert_eq!(mob.drain_sorted_asks(), vec![]);
    }

    #[test]
    fn test_merged_order_book_multiple_snapshots_old_entries_removed() {
        let bitstamp = "bitstamp";
        let binance_com = "binance_com";
        let mut mob = MergedOrderBook::new();
        let snap1 = OrderBookSnapshot{
            bids: vec![[dec!(10), dec!(1)],
                       [dec!(9), dec!(1)],
                       [dec!(8), dec!(1)],
                       [dec!(7), dec!(1)],
                       [dec!(6), dec!(1)],
                       [dec!(5), dec!(1)],
                       [dec!(4), dec!(1)],
                       [dec!(3), dec!(1)],
                       [dec!(2), dec!(1)],
                       [dec!(1), dec!(1)]], 
            asks: vec![[dec!(11), dec!(1)],
                       [dec!(12), dec!(1)],
                       [dec!(13), dec!(1)],
                       [dec!(14), dec!(1)],
                       [dec!(15), dec!(1)],
                       [dec!(16), dec!(1)],
                       [dec!(17), dec!(1)],
                       [dec!(18), dec!(1)],
                       [dec!(19), dec!(1)],
                       [dec!(20), dec!(1)]], 
            exchange_name: bitstamp.to_string(),
            symbol: "btcusd".to_string()};

        mob.merge_snapshot(snap1);   

        let snap2 = OrderBookSnapshot{
            bids: vec![[dec!(10), dec!(2)],
                        [dec!(9), dec!(2)],
                        [dec!(8), dec!(2)],
                        [dec!(7), dec!(2)],
                        [dec!(6), dec!(1)],
                        [dec!(5), dec!(2)],
                        [dec!(4), dec!(2)],
                        [dec!(3), dec!(2)],
                        [dec!(2), dec!(2)],
                        [dec!(1), dec!(2)]], 
            asks: vec![[dec!(11), dec!(2)],
                        [dec!(12), dec!(2)],
                        [dec!(13), dec!(2)],
                        [dec!(14), dec!(2)],
                        [dec!(15), dec!(2)],
                        [dec!(16), dec!(2)],
                        [dec!(17), dec!(2)],
                        [dec!(18), dec!(2)],
                        [dec!(19), dec!(2)],
                        [dec!(20), dec!(2)]], 
            exchange_name: binance_com.to_string(),
            symbol: "btcusd".to_string()};

            mob.merge_snapshot(snap2);

            let snap3 = OrderBookSnapshot{
                bids: vec![[dec!(5), dec!(2)]], 
                asks: vec![[dec!(16), dec!(2)]], 
                exchange_name: binance_com.to_string(),
                symbol: "btcusd".to_string()};

            mob.merge_snapshot(snap3);

            // check that old binance_com snapshot entries are gone
            assert_eq!(mob.drain_sorted_bids(), vec![
                Level{price: dec!(10), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(9), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(8), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(7), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(6), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(5), qty: dec!(2), exchange_name: binance_com.to_string()}]);
            assert_eq!(mob.drain_sorted_asks(), vec![
                Level{price: dec!(11), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(12), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(13), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(14), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(15), qty: dec!(1), exchange_name: bitstamp.to_string()},
                Level{price: dec!(16), qty: dec!(2), exchange_name: binance_com.to_string()},]);

            assert_eq!(mob.drain_sorted_bids(), vec![]);
            assert_eq!(mob.drain_sorted_asks(), vec![]);
    }
}
