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
