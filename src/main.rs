use std::collections::hash_map::Entry;
use std::process::ExitCode;
use std::{
    collections::HashMap,
    io::{BufWriter, IsTerminal},
    sync::Mutex,
};

use client_record::{ClientId, ClientRecord};
use transaction::Transaction;

mod client_record;
mod transaction;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(input_file) = std::env::args_os().nth(1) else {
        tracing::error!("No input file was provided");
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).into());
    };

    let mut clients = HashMap::<ClientId, ClientRecord>::new();

    let mut rdr = transaction::csv_reader_builder().from_path(input_file)?;
    let headers = rdr.headers()?.clone();
    let mut record = csv::StringRecord::new();
    while rdr.read_record(&mut record)? {
        // A line of only whitespace trims to empty fields; skip it like an empty line.
        if record.iter().all(str::is_empty) {
            continue;
        }
        let transaction: Transaction = record.deserialize(Some(&headers))?;
        match clients.entry(transaction.client) {
            Entry::Occupied(oe) => {
                let client = oe.into_mut();
                if let Err(err) = client.update(&transaction) {
                    tracing::error!("transaction failed: {err}")
                }
            }
            Entry::Vacant(ve) => {
                let mut client = ClientRecord::new(transaction.client);
                if let Err(err) = client.update(&transaction) {
                    tracing::error!("transaction failed: {err}")
                } else {
                    ve.insert(client);
                }
            }
        }
    }

    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(std::io::stdout().lock());
    // Written by hand so the header is present even when there are no accounts.
    writer.write_record(ClientRecord::CSV_HEADER)?;
    for client in clients.values() {
        writer.serialize(client)?;
    }
    writer.flush()?;

    Ok(())
}

fn main() -> ExitCode {
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(std::io::stderr().is_terminal())
        .with_writer(Mutex::new(BufWriter::new(std::io::stderr())))
        .finish();
    // Dropping the guard when `main` returns drops the buffer, which flushes it.
    let _log_guard = tracing::subscriber::set_default(subscriber);

    if let Err(err) = run() {
        tracing::error!("execution failed: {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
