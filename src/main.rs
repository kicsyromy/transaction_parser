use transaction::Transaction;

mod transaction;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Hello, world!");

    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path("test_input.csv")?;
    for result in rdr.deserialize() {
        let record: Transaction = result?;
        println!("{:?}", record);
    }

    Ok(())
}
