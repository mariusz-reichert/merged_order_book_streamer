#[macro_use]
extern crate serde;
extern crate serde_json;

pub mod core {

    const TOP_K: usize = 10;

    use std::{
        fs::File,
        io::{prelude::*, BufReader},
        path::Path,
    };
    use rust_decimal::Decimal;
    use std::collections::BinaryHeap;
    use std::cmp::{Ordering, Ord, PartialOrd, PartialEq, Eq};
    use std::hash::{Hash, Hasher};

    #[derive(Debug, Deserialize, Serialize)]
    pub struct Config {
        pub exchanges: Vec<Exchange>
    }
    
    #[derive(Debug, Deserialize, Serialize)]
    pub struct Exchange {
        pub name: String,
        pub is_enabled: bool,
        pub symbols_url:String,
        pub api_url: String
        //pub ws: 
    }

    impl Hash for Exchange {
        #[inline]
        fn hash<H: Hasher>(&self, hasher: &mut H) {
            self.name.hash(hasher);
        }
    }

    impl PartialEq for Exchange {
        #[inline]
        fn eq(&self, other: &Self) -> bool {
            self.name == other.name
        }
    }

    impl Eq for Exchange {}
    
    pub fn read_config(path: &str) -> std::io::Result<Config> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    // todo: rewrite it into traits
    pub fn get_json_symbols(ex_name: &str, json: &serde_json::Value) -> Vec<String> {
        let mut result = Vec::new();
        match ex_name {
            "binance_com" => {
                let symbols = json.get("symbols").unwrap().as_array().unwrap();
                for s in symbols {
                    result.push(format!("{}{}", s.get("baseAsset").unwrap().as_str().unwrap().to_lowercase(), s.get("quoteAsset").unwrap().as_str().unwrap().to_lowercase()));
                }
            },
            "bitstamp" => {
                let symbols = json.as_array().unwrap();
                for s in symbols {
                    result.push(s.get("url_symbol").unwrap().as_str().unwrap().to_string());
                }
            },
            _ => {}
        }
        result
    }

    pub fn load_symbols_from_file(filename: impl AsRef<Path>) -> Vec<String> {
        let file = File::open(filename).expect("No such file");
        let buf = BufReader::new(file);
        buf.lines()
            .map(|l| l.expect("Could not parse line"))
            .collect()
    }

    // todo: rewrite it into traits
    pub fn build_subscribe_msgs(ex: &Exchange, symbols: &Vec<String>) -> Vec<String> {
        let mut subscribe_msgs : Vec<String> = Vec::new();
        match &ex.name[..] {
            "binance_com" => {
                let mut channels = String::new();
                for s in symbols {
                    channels.push_str(format!("\"{}@depth20@100ms\",", &s).as_str());
                }
                // remove trailing ','
                channels.pop();
                subscribe_msgs.push(format!("{{\"method\":\"SUBSCRIBE\",\"params\":[{}],\"id\":1}}", channels));
            },
            "bitstamp" => {
                for s in symbols {
                    subscribe_msgs.push(format!("{{\"event\":\"bts:subscribe\",\"data\":{{\"channel\":\"order_book_{}\"}}}}", &s));
                }
            },
            _ => { }
        }
        subscribe_msgs
    }
    
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

