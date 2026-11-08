use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
    #[serde(untagged)]
    Unknown(String),
}

#[repr(transparent)]
#[derive(Debug, Serialize, Deserialize)]
pub struct ClientId(u16);

#[repr(transparent)]
#[derive(Debug, Serialize, Deserialize)]
pub struct Id(u32);

#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    #[serde(rename = "type")]
    kind: Kind,
    client: ClientId,
    tx: Id,
    #[serde(with = "rust_decimal::serde::str_option")]
    amount: Option<Decimal>,
}
