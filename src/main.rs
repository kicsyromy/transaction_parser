use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::env::args;

use client_record::{ClientId, ClientRecord};
use transaction::Transaction;

mod client_record;
mod transaction;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(input_file) = args().skip(1).next() else {
        tracing::error!("No input file was provided");
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).into());
    };

    let mut clients = HashMap::<ClientId, ClientRecord>::new();

    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path(input_file)?;
    for result in rdr.deserialize() {
        let transaction: Transaction = result?;
        match clients.entry(transaction.client) {
            Entry::Occupied(oe) => {
                let client = oe.into_mut();
                client.update(&transaction)?;
            }
            Entry::Vacant(ve) => {
                let client = ve.insert(ClientRecord::new(transaction.client));
                client.update(&transaction)?;
            }
        }
    }

    Ok(())
}

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    run().expect("application execution failed");
}
