use std::{
    error::Error,
    fs::{File, OpenOptions},
    io::BufReader,
    path::Path,
    string::String,
    thread,
    time::Duration
};

use chrono::prelude::*;
use csv::Writer;
use env_logger::{Builder, Target, WriteStyle};
use ftx::{
    options::{Endpoint, Options},
    rest::{GetFuture, GetOrderBook, Rest},
};
use log::{debug, error, info, LevelFilter, warn};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use serde_json;
use ta::{indicators::BollingerBands, Next};

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    market_name: String,
    time_delta: u64,
    bb_period: usize,
    bb_std_dev: f64,
    orderbook_depth: u32,
    live: bool,
    positions_filename: String,
}

fn write_to_csv(filename: &str, price: &f64, position: &str) -> Result<(), Box<dyn Error>> {
    /* Write utc time, price and position to a csv file */
    let utc_time: DateTime<Utc> = Utc::now();

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .append(true)
        .open(String::from(filename))
        .unwrap();
    let mut wtr = Writer::from_writer(file);
    debug!("Writing position to {:?}", String::from(filename));
    wtr.write_record(&[utc_time.to_string(), price.to_string(), String::from(position)])?;
    wtr.flush()?;
    Ok(())
}

#[tokio::main]
async fn main() {
    // Load configuration file
    let settings_filepath = Path::new("settings.json");
    let settings_file = File::open(settings_filepath).expect("Config file not found");
    let reader = BufReader::new(settings_file);
    let settings: SettingsFile =
        serde_json::from_reader(reader).expect("Error when reading config json");

    let mut builder = Builder::new();
    builder
        .filter(None, LevelFilter::Info)
        .write_style(WriteStyle::Always)
        .target(Target::Stdout)
        .init();

    info!("Settings file loaded from {:?}.", settings_filepath);
    info!(
        "market_name={:?}, time_delta={:?}, bb_period={:?}, bb_std_dev={:?}, orderbook_depth={:?}, \
        positions_filename={:?}",
        String::from(&settings.market_name),
        settings.time_delta,
        settings.bb_period,
        settings.bb_std_dev,
        settings.orderbook_depth,
        settings.positions_filename
    );
    if settings.live {
        warn!("The bot is running live")
    }
    info!("Setting trigger in {:?} iterations (approx {:?}s)...",
        settings.bb_period, settings.bb_period * settings.time_delta.to_usize().unwrap());

    // Set up connection to FTX API
    let api = if settings.live {
        Rest::new(Options::from_env())
    } else {
        Rest::new(Options { endpoint: Endpoint::Com, ..Default::default() })
    };

    // Set up bollinger bands
    let mut bb = BollingerBands::new(settings.bb_period, settings.bb_std_dev).unwrap();

    let mut count: usize = 0;

    loop {
        count += 1;

        // Get orderbook and handle error
        let order_book = api.request(
            GetOrderBook {
                market_name: String::from(&settings.market_name),
                depth: Option::from(settings.orderbook_depth),
            }
        ).await;
        let order_book = match order_book {
            Err(e) => {
                // Continue loop is getting orderbook fails
                error!("Error: {:?}", e);
                continue;
            }
            Ok(o) => o
        };

        // Calculate values used for analysis
        let perp_delta = (order_book.bids[0].1 - order_book.asks[0].1).to_f64().unwrap();
        let out = bb.next(perp_delta);
        let bb_lower = out.lower;
        let bb_upper = out.upper;

        debug!("perp_delta={:.2}, bb_lower={:.2}, bb_upper={:.2}", perp_delta, bb_lower, bb_upper);

        if count > settings.bb_period {
            if count == settings.bb_period + 1 {
                warn!("Trigger is now set...")
            }

            if perp_delta > bb_upper || perp_delta < bb_lower {
                // Get price and handle error
                let price = api.request(
                    GetFuture { future_name: String::from(&settings.market_name) }
                ).await;
                let btc_price = match price {
                    Err(e) => {
                        error!("Error: {:?}", e);
                        continue;
                    }
                    Ok(o) => o
                };

                let mut price: f64 = 0.0;
                let mut position: String = "none".to_string();

                if perp_delta > bb_upper {
                    // Enter long position
                    price = btc_price.ask.unwrap().to_f64().unwrap();
                    position = String::from("long");
                    warn!("Perp delta above upper bb, going {} at {:.2}", position, price);
                } else if perp_delta < bb_lower {
                    // Enter short position
                    price = btc_price.bid.unwrap().to_f64().unwrap();
                    position = String::from("short");
                    warn!("Perp delta below lower bb, going {} at {:.2}", position, price);
                }

                // Write the positions to a csv
                write_to_csv(
                    &settings.positions_filename,
                    &price,
                    &position,
                ).expect("Unable to write positions to file.");
            }
        }
        thread::sleep(Duration::from_secs(settings.time_delta));
    }
}
