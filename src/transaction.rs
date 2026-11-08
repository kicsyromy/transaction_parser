use std::fmt::Display;

use rust_decimal::Decimal;
use serde::Deserialize;

use crate::ClientId;

#[derive(Debug, Copy, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
    #[serde(other)]
    Unknown,
}

#[repr(transparent)]
#[derive(Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Copy, Clone, Deserialize)]
pub struct Id(u32);

impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}", self.0))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Transaction {
    #[serde(rename = "type")]
    pub kind: Kind,
    pub client: ClientId,
    pub tx: Id,
    #[serde(with = "rust_decimal::serde::str_option")]
    pub amount: Option<Decimal>,
}

/// CSV settings for transaction input: whitespace is trimmed and the amount column may be omitted.
pub fn csv_reader_builder() -> csv::ReaderBuilder {
    let mut builder = csv::ReaderBuilder::new();
    builder.trim(csv::Trim::All).flexible(true);
    builder
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn parse(input: &str) -> Vec<Transaction> {
        csv_reader_builder()
            .from_reader(input.as_bytes())
            .deserialize()
            .collect::<Result<_, _>>()
            .expect("input parses")
    }

    fn parse_kind(name: &str) -> Kind {
        parse(&format!("type,client,tx,amount\n{name},1,1,1\n"))[0].kind
    }

    #[test]
    fn whitespace_is_trimmed() {
        let rows = parse("  type ,client,  tx,amount\n  deposit ,  1 ,  2 ,  3.5  \n");
        assert!(matches!(rows[0].kind, Kind::Deposit));
        assert_eq!(rows[0].tx, Id(2));
        assert_eq!(rows[0].amount, Some(Decimal::from_str("3.5").unwrap()));
    }

    #[test]
    fn empty_amount_is_none() {
        let rows = parse("type,client,tx,amount\ndispute,1,2,\n");
        assert_eq!(rows[0].amount, None);
    }

    #[test]
    fn omitted_amount_column_is_none() {
        let rows = parse("type,client,tx,amount\ndispute,1,2\n");
        assert_eq!(rows[0].amount, None);
    }

    #[test]
    fn lowercase_names_map_to_kinds() {
        assert!(matches!(parse_kind("deposit"), Kind::Deposit));
        assert!(matches!(parse_kind("withdrawal"), Kind::Withdrawal));
        assert!(matches!(parse_kind("dispute"), Kind::Dispute));
        assert!(matches!(parse_kind("resolve"), Kind::Resolve));
        assert!(matches!(parse_kind("chargeback"), Kind::Chargeback));
    }

    #[test]
    fn other_names_are_unknown() {
        for name in ["Deposit", "transfer", "7"] {
            assert!(matches!(parse_kind(name), Kind::Unknown), "{name}");
        }
    }

    #[test]
    fn amounts_keep_all_decimal_places() {
        let rows = parse("type,client,tx,amount\ndeposit,1,1,1.123456\n");
        let amount = rows[0].amount.expect("amount present");
        assert_eq!(amount, Decimal::from_str("1.123456").unwrap());
        assert_eq!(amount.scale(), 6);
    }
}
