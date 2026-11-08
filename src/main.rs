use std::env::args;

use transaction::Transaction;

mod transaction;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(input_file) = args().skip(1).next() else {
        tracing::error!("No input file was provided");
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput).into());
    };

    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path("test_input.csv")?;
    for result in rdr.deserialize() {
        let record: Transaction = result?;
        println!("{:?}", record);
    }

    Ok(())
}

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    run().expect("application execution failed");
}
