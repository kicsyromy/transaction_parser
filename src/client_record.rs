use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Transaction;
use crate::transaction::Id as TransactionId;
use crate::transaction::Kind as TransactionKind;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("transaction {0}, type {1:?}, is missing an expected amount")]
    MissingAmount(TransactionId, TransactionKind),
    #[error("insufficient funds for transaction {0}, type withdrawal")]
    InsufficientFunds(TransactionId),
}

#[repr(transparent)]
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct ClientId(u16);

#[derive(Debug, Serialize)]
pub struct ClientRecord {
    client: ClientId,
    #[serde(with = "rust_decimal::serde::str")]
    available: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    held: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    total: Decimal,
    locked: bool,

    #[serde(skip)]
    transaction_history: HashMap<TransactionId, Transaction>,
    #[serde(skip)]
    disputed_transactions: HashSet<TransactionId>,
}

impl ClientRecord {
    pub fn new(id: ClientId) -> Self {
        Self {
            client: id,
            available: 0.into(),
            held: 0.into(),
            total: 0.into(),
            locked: false,

            transaction_history: HashMap::new(),
            disputed_transactions: HashSet::new(),
        }
    }

    pub fn update(&mut self, transaction: &Transaction) -> Result<(), UpdateError> {
        self.transaction_history
            .insert(transaction.tx, transaction.clone());

        match transaction.kind {
            TransactionKind::Deposit => {
                let Some(amount) = transaction.amount else {
                    return Err(UpdateError::MissingAmount(
                        transaction.tx,
                        transaction.kind.clone(),
                    ));
                };

                self.available += amount;
                self.total += amount;
            }
            TransactionKind::Withdrawal => {
                let Some(amount) = transaction.amount else {
                    return Err(UpdateError::MissingAmount(
                        transaction.tx,
                        transaction.kind.clone(),
                    ));
                };

                if self.available < amount {
                    return Err(UpdateError::InsufficientFunds(transaction.tx));
                }

                self.available -= amount;
            }
            TransactionKind::Dispute => {
                let Some(disputed) = self.transaction_history.get(&transaction.tx) else {
                    tracing::warn!("tried to dispute invalid transaction id {}", transaction.tx);
                    return Ok(());
                };

                let Some(amount) = disputed.amount else {
                    return Err(UpdateError::MissingAmount(
                        disputed.tx,
                        disputed.kind.clone(),
                    ));
                };

                self.disputed_transactions.insert(disputed.tx);

                self.available -= amount;
                self.held += amount;
            }
            TransactionKind::Resolve => {
                let Some(resolved) = self.transaction_history.get(&transaction.tx) else {
                    tracing::warn!("tried to resolve invalid transaction id {}", transaction.tx);
                    return Ok(());
                };

                if !self.disputed_transactions.contains(&resolved.tx) {
                    tracing::warn!(
                        "tried to resolve an undisputed transaction id {}",
                        transaction.tx
                    );
                    return Ok(());
                }

                let Some(amount) = resolved.amount else {
                    return Err(UpdateError::MissingAmount(
                        resolved.tx,
                        resolved.kind.clone(),
                    ));
                };

                self.available += amount;
                self.held -= amount;
            }
            TransactionKind::Chargeback => {
                let Some(resolved) = self.transaction_history.get(&transaction.tx) else {
                    tracing::warn!("tried to resolve invalid transaction id {}", transaction.tx);
                    return Ok(());
                };

                if !self.disputed_transactions.contains(&resolved.tx) {
                    tracing::warn!(
                        "tried to chargeback an undisputed transaction id {}",
                        transaction.tx
                    );
                    return Ok(());
                }

                let Some(amount) = resolved.amount else {
                    return Err(UpdateError::MissingAmount(
                        resolved.tx,
                        resolved.kind.clone(),
                    ));
                };

                self.held -= amount;
                self.total -= amount;
                self.locked = true;
            }
            TransactionKind::Unknown(_) => {}
        }

        Ok(())
    }
}
