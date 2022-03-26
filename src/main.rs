use std::{thread, time::Duration};

use csv::Writer;
use ftx::{
    options::{Endpoint, Options},
    rest::{GetFuture, GetOrderBook, Rest, Result},
};
use ftx::ws::Channel::Orderbook;
use rust_decimal::prelude::ToPrimitive;
use ta::indicators::BollingerBands;
use ta::Next;

#[tokio::main]
async fn main() -> Result<()> {
    // Time gap in seconds between points
    const TIME_DELTA: u64 = 5;
    // Period and standard deviation of bollinger bands
    const BB_PERIOD: usize = 20;
    const BB_STD_DEV: f64 = 2.0;

    // Set up connection to FTX API
    let api = Rest::new(
        Options { endpoint: Endpoint::Com, ..Default::default() }
    );

    // Set up bollinger bands
    let mut bb = BollingerBands::new(BB_PERIOD, BB_STD_DEV).unwrap();

    let mut count: usize = 0;

    loop {
        count += 1;
        let order_book = api.request(
            GetOrderBook { market_name: "BTC-PERP".to_string(), depth: 1.to_u32() }
        ).await?;

        let perp_delta = (order_book.bids[0].1 - order_book.asks[0].1).to_f64().unwrap();

        let out = bb.next(perp_delta);
        let bb_lower = out.lower;
        let bb_upper = out.upper;

        println!("perp_delta={:.2}, bb_lower={:.2}, bb_upper={:.2}", perp_delta, bb_lower, bb_upper);

        if count > BB_PERIOD {
            if count == BB_PERIOD + 1 {
                println!("Trigger is now set...")
            }

            if perp_delta > bb_upper || perp_delta < bb_lower {
                let btc_price = api.request(
                    GetFuture { future_name: "BTC-PERP".to_string() }
                ).await?;
                let price: f64 = 0.0;
                let position: String = "none".to_string();

                if perp_delta > bb_upper {
                    let price = btc_price.bid.unwrap().to_f64().unwrap();
                    let position = "short";
                    println!("Perp delta above upper bb, going {} at {:.2}", position, price);
                } else {
                    let price = btc_price.bid.unwrap().to_f64().unwrap();
                    let position = "long";
                    println!("Perp delta below lower bb, going {} at {:.2}", position, price);
                }
            }
        }
        thread::sleep(Duration::from_secs(TIME_DELTA));
    }
    Ok(())
}
