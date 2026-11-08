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
    #[error("amount for transaction {0} is negative")]
    NegativeAmount(TransactionId),
    #[error("dispute for withdrawal transaction {0}")]
    DisputeWithdrawal(TransactionId),
    #[error("transaction {0} is already being disputed")]
    AlreadyDisputed(TransactionId),
    #[error("transaction {0}, type {1:?}, failed: account frozen")]
    AccountFrozen(TransactionId, TransactionKind),
    #[error("transaction {0} has an unknown type")]
    UnknownTransactionType(TransactionId),
    #[error("transaction {0} would overflow/underflow the balance")]
    Overflow(TransactionId),
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
    transaction_history: Vec<Transaction>,
    #[serde(skip)]
    disputable_transactions: HashMap<TransactionId, usize>,
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

            transaction_history: Vec::new(),
            disputable_transactions: HashMap::new(),
            disputed_transactions: HashSet::new(),
        }
    }

    pub fn update(&mut self, transaction: &Transaction) -> Result<(), UpdateError> {
        self.transaction_history.push(transaction.clone());

        if self.locked {
            return Err(UpdateError::AccountFrozen(transaction.tx, transaction.kind));
        }

        match transaction.kind {
            TransactionKind::Deposit => {
                let Some(amount) = transaction.amount else {
                    return Err(UpdateError::MissingAmount(transaction.tx, transaction.kind));
                };

                if amount.is_sign_negative() {
                    return Err(UpdateError::NegativeAmount(transaction.tx));
                }

                let Some(available) = self.available.checked_add(amount) else {
                    return Err(UpdateError::Overflow(transaction.tx));
                };
                self.available = available;

                self.disputable_transactions
                    .insert(transaction.tx, self.transaction_history.len() - 1);
            }
            TransactionKind::Withdrawal => {
                let Some(amount) = transaction.amount else {
                    return Err(UpdateError::MissingAmount(transaction.tx, transaction.kind));
                };

                if amount.is_sign_negative() {
                    return Err(UpdateError::NegativeAmount(transaction.tx));
                }

                if self.available < amount {
                    return Err(UpdateError::InsufficientFunds(transaction.tx));
                }

                let Some(available) = self.available.checked_sub(amount) else {
                    return Err(UpdateError::Overflow(transaction.tx));
                };
                self.available = available;

                self.disputable_transactions
                    .insert(transaction.tx, self.transaction_history.len() - 1);
            }
            TransactionKind::Dispute => {
                let Some(disputed) = self
                    .disputable_transactions
                    .get(&transaction.tx)
                    .map(|&i| &self.transaction_history[i])
                else {
                    tracing::warn!("tried to dispute invalid transaction id {}", transaction.tx);
                    return Ok(());
                };

                if !matches!(disputed.kind, TransactionKind::Deposit) {
                    return Err(UpdateError::DisputeWithdrawal(transaction.tx));
                }

                let Some(amount) = disputed.amount else {
                    return Err(UpdateError::MissingAmount(disputed.tx, disputed.kind));
                };

                if !self.disputed_transactions.insert(disputed.tx) {
                    return Err(UpdateError::AlreadyDisputed(disputed.tx));
                }

                let Some(available) = self.available.checked_sub(amount) else {
                    return Err(UpdateError::Overflow(transaction.tx));
                };
                self.available = available;

                let Some(held) = self.held.checked_add(amount) else {
                    return Err(UpdateError::Overflow(transaction.tx));
                };
                self.held = held;
            }
            TransactionKind::Resolve => {
                let Some(resolved) = self
                    .disputable_transactions
                    .get(&transaction.tx)
                    .map(|&i| &self.transaction_history[i])
                else {
                    tracing::warn!(
                        "tried to resolve undisputable transaction id {}",
                        transaction.tx
                    );
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
                    return Err(UpdateError::MissingAmount(resolved.tx, resolved.kind));
                };

                let Some(available) = self.available.checked_add(amount) else {
                    return Err(UpdateError::Overflow(transaction.tx));
                };
                self.available = available;

                let Some(held) = self.held.checked_sub(amount) else {
                    return Err(UpdateError::Overflow(transaction.tx));
                };
                self.held = held;

                self.disputed_transactions.remove(&transaction.tx);
                self.disputable_transactions.remove(&transaction.tx);
            }
            TransactionKind::Chargeback => {
                let Some(charged_back) = self
                    .disputable_transactions
                    .get(&transaction.tx)
                    .map(|&i| &self.transaction_history[i])
                else {
                    tracing::warn!(
                        "tried to chargeback undisputable transaction id {}",
                        transaction.tx
                    );
                    return Ok(());
                };

                if !self.disputed_transactions.contains(&charged_back.tx) {
                    tracing::warn!(
                        "tried to chargeback an undisputed transaction id {}",
                        transaction.tx
                    );
                    return Ok(());
                }

                let Some(amount) = charged_back.amount else {
                    return Err(UpdateError::MissingAmount(
                        charged_back.tx,
                        charged_back.kind,
                    ));
                };

                let Some(held) = self.held.checked_sub(amount) else {
                    return Err(UpdateError::Overflow(transaction.tx));
                };
                self.held = held;

                self.locked = true;

                self.disputed_transactions.remove(&transaction.tx);
                self.disputable_transactions.remove(&transaction.tx);
            }
            TransactionKind::Unknown => {
                return Err(UpdateError::UnknownTransactionType(transaction.tx));
            }
        }

        // Make sure `total` is always correct
        let Some(total) = self.available.checked_add(self.held) else {
            return Err(UpdateError::Overflow(transaction.tx));
        };
        self.total = total;

        Ok(())
    }
}
