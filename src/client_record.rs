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
#[cfg_attr(test, derive(Clone, PartialEq))]
pub struct ClientRecord {
    client: ClientId,
    #[serde(serialize_with = "serialize_amount")]
    available: Decimal,
    #[serde(serialize_with = "serialize_amount")]
    held: Decimal,
    #[serde(serialize_with = "serialize_amount")]
    total: Decimal,
    locked: bool,

    /// Successful deposits that can still be disputed.
    #[serde(skip)]
    deposits: HashMap<TransactionId, Decimal>,
    /// Successful withdrawals, kept so a dispute of one is reported as such.
    #[serde(skip)]
    withdrawals: HashSet<TransactionId>,
    /// Deposits currently under dispute.
    #[serde(skip)]
    disputed: HashSet<TransactionId>,
}

impl ClientRecord {
    pub const CSV_HEADER: [&str; 5] = ["client", "available", "held", "total", "locked"];

    pub fn new(id: ClientId) -> Self {
        Self {
            client: id,
            available: 0.into(),
            held: 0.into(),
            total: 0.into(),
            locked: false,

            deposits: HashMap::new(),
            withdrawals: HashSet::new(),
            disputed: HashSet::new(),
        }
    }

    pub fn update(&mut self, transaction: &Transaction) -> Result<(), UpdateError> {
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
                self.set_balances(transaction.tx, available, self.held)?;

                self.deposits.insert(transaction.tx, amount);
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
                self.set_balances(transaction.tx, available, self.held)?;

                self.withdrawals.insert(transaction.tx);
            }
            TransactionKind::Dispute => {
                let Some(&amount) = self.deposits.get(&transaction.tx) else {
                    if self.withdrawals.contains(&transaction.tx) {
                        return Err(UpdateError::DisputeWithdrawal(transaction.tx));
                    }
                    tracing::warn!("tried to dispute invalid transaction id {}", transaction.tx);
                    return Ok(());
                };

                if self.disputed.contains(&transaction.tx) {
                    return Err(UpdateError::AlreadyDisputed(transaction.tx));
                }

                let (Some(available), Some(held)) = (
                    self.available.checked_sub(amount),
                    self.held.checked_add(amount),
                ) else {
                    return Err(UpdateError::Overflow(transaction.tx));
                };
                self.set_balances(transaction.tx, available, held)?;

                self.disputed.insert(transaction.tx);
            }
            TransactionKind::Resolve => {
                let Some(&amount) = self.deposits.get(&transaction.tx) else {
                    tracing::warn!(
                        "tried to resolve undisputable transaction id {}",
                        transaction.tx
                    );
                    return Ok(());
                };

                if !self.disputed.contains(&transaction.tx) {
                    tracing::warn!(
                        "tried to resolve an undisputed transaction id {}",
                        transaction.tx
                    );
                    return Ok(());
                }

                let (Some(available), Some(held)) = (
                    self.available.checked_add(amount),
                    self.held.checked_sub(amount),
                ) else {
                    return Err(UpdateError::Overflow(transaction.tx));
                };
                self.set_balances(transaction.tx, available, held)?;

                self.disputed.remove(&transaction.tx);
                self.deposits.remove(&transaction.tx);
            }
            TransactionKind::Chargeback => {
                let Some(&amount) = self.deposits.get(&transaction.tx) else {
                    tracing::warn!(
                        "tried to chargeback undisputable transaction id {}",
                        transaction.tx
                    );
                    return Ok(());
                };

                if !self.disputed.contains(&transaction.tx) {
                    tracing::warn!(
                        "tried to chargeback an undisputed transaction id {}",
                        transaction.tx
                    );
                    return Ok(());
                }

                let Some(held) = self.held.checked_sub(amount) else {
                    return Err(UpdateError::Overflow(transaction.tx));
                };
                self.set_balances(transaction.tx, self.available, held)?;

                self.locked = true;

                // A locked account rejects every later row, so nothing stored is read again.
                self.deposits = HashMap::new();
                self.withdrawals = HashSet::new();
                self.disputed = HashSet::new();
            }
            TransactionKind::Unknown => {
                return Err(UpdateError::UnknownTransactionType(transaction.tx));
            }
        }

        Ok(())
    }

    /// Sets both balances and `total` together, or changes nothing if `total` would overflow.
    fn set_balances(
        &mut self,
        tx: TransactionId,
        available: Decimal,
        held: Decimal,
    ) -> Result<(), UpdateError> {
        let Some(total) = available.checked_add(held) else {
            return Err(UpdateError::Overflow(tx));
        };
        self.available = available;
        self.held = held;
        self.total = total;
        Ok(())
    }
}

/// Four decimal places, rounded half to even. Values too large for four places keep as many as fit.
fn format_amount(amount: Decimal) -> String {
    let mut rounded = amount.round_dp(4);
    rounded.rescale(4);
    rounded.to_string()
}

fn serialize_amount<S: serde::Serializer>(
    amount: &Decimal,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format_amount(*amount))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::transaction::csv_reader_builder;

    fn tx(row: &str) -> Transaction {
        let input = format!("type,client,tx,amount\n{row}\n");
        csv_reader_builder()
            .from_reader(input.as_bytes())
            .deserialize::<Transaction>()
            .next()
            .expect("input has a row")
            .expect("row parses")
    }

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).expect("valid decimal")
    }

    fn new_record() -> ClientRecord {
        ClientRecord::new(ClientId(1))
    }

    /// Applies every row in order, ignoring individual results.
    fn apply(rows: &[&str]) -> ClientRecord {
        let mut record = new_record();
        for row in rows {
            let _ = record.update(&tx(row));
        }
        record
    }

    #[track_caller]
    fn assert_account(
        record: &ClientRecord,
        available: &str,
        held: &str,
        total: &str,
        locked: bool,
    ) {
        assert_eq!(
            (record.available, record.held, record.total, record.locked),
            (dec(available), dec(held), dec(total), locked)
        );
    }

    #[test]
    fn deposit_credits_available_and_total() {
        let record = apply(&["deposit, 1, 1, 10.5"]);
        assert_account(&record, "10.5", "0", "10.5", false);
    }

    #[test]
    fn deposit_without_amount_is_rejected() {
        let mut record = new_record();
        let result = record.update(&tx("deposit, 1, 1,"));
        assert!(matches!(result, Err(UpdateError::MissingAmount(..))));
        assert_account(&record, "0", "0", "0", false);
    }

    #[test]
    fn negative_deposit_is_rejected() {
        let mut record = apply(&["deposit, 1, 1, 10"]);
        let result = record.update(&tx("deposit, 1, 2, -5"));
        assert!(matches!(result, Err(UpdateError::NegativeAmount(_))));
        assert_account(&record, "10", "0", "10", false);
    }

    #[test]
    fn withdrawal_debits_available_and_total() {
        let record = apply(&["deposit, 1, 1, 10", "withdrawal, 1, 2, 4.25"]);
        assert_account(&record, "5.75", "0", "5.75", false);
    }

    #[test]
    fn withdrawal_of_entire_balance_is_allowed() {
        let mut record = apply(&["deposit, 1, 1, 10"]);
        assert!(record.update(&tx("withdrawal, 1, 2, 10")).is_ok());
        assert_account(&record, "0", "0", "0", false);
    }

    #[test]
    fn withdrawal_with_insufficient_funds_is_rejected() {
        let mut record = apply(&["deposit, 1, 1, 10"]);
        let result = record.update(&tx("withdrawal, 1, 2, 10.0001"));
        assert!(matches!(result, Err(UpdateError::InsufficientFunds(_))));
        assert_account(&record, "10", "0", "10", false);
    }

    #[test]
    fn held_funds_cannot_be_withdrawn() {
        let mut record = apply(&["deposit, 1, 1, 10", "dispute, 1, 1,"]);
        let result = record.update(&tx("withdrawal, 1, 2, 5"));
        assert!(matches!(result, Err(UpdateError::InsufficientFunds(_))));
        assert_account(&record, "0", "10", "10", false);
    }

    #[test]
    fn negative_withdrawal_is_rejected() {
        let mut record = apply(&["deposit, 1, 1, 10"]);
        let result = record.update(&tx("withdrawal, 1, 2, -5"));
        assert!(matches!(result, Err(UpdateError::NegativeAmount(_))));
        assert_account(&record, "10", "0", "10", false);
    }

    #[test]
    fn dispute_moves_funds_to_held() {
        let record = apply(&["deposit, 1, 1, 10", "deposit, 1, 2, 5", "dispute, 1, 1,"]);
        assert_account(&record, "5", "10", "15", false);
    }

    #[test]
    fn dispute_of_unknown_transaction_is_ignored() {
        let mut record = apply(&["deposit, 1, 1, 10"]);
        assert!(record.update(&tx("dispute, 1, 99,")).is_ok());
        assert_account(&record, "10", "0", "10", false);
    }

    #[test]
    fn dispute_of_withdrawal_is_rejected() {
        let mut record = apply(&["deposit, 1, 1, 10", "withdrawal, 1, 2, 4"]);
        let result = record.update(&tx("dispute, 1, 2,"));
        assert!(matches!(result, Err(UpdateError::DisputeWithdrawal(_))));
        assert_account(&record, "6", "0", "6", false);
    }

    #[test]
    fn dispute_of_failed_withdrawal_is_ignored() {
        let mut record = apply(&["deposit, 1, 1, 10", "withdrawal, 1, 2, 50"]);
        assert!(record.update(&tx("dispute, 1, 2,")).is_ok());
        assert_account(&record, "10", "0", "10", false);
    }

    #[test]
    fn second_dispute_is_rejected() {
        let mut record = apply(&["deposit, 1, 1, 10", "dispute, 1, 1,"]);
        let result = record.update(&tx("dispute, 1, 1,"));
        assert!(matches!(result, Err(UpdateError::AlreadyDisputed(_))));
        assert_account(&record, "0", "10", "10", false);
    }

    #[test]
    fn dispute_can_make_available_negative() {
        let record = apply(&[
            "deposit, 1, 1, 10",
            "withdrawal, 1, 2, 10",
            "dispute, 1, 1,",
        ]);
        assert_account(&record, "-10", "10", "0", false);
    }

    #[test]
    fn resolve_releases_held_funds() {
        let record = apply(&["deposit, 1, 1, 10", "dispute, 1, 1,", "resolve, 1, 1,"]);
        assert_account(&record, "10", "0", "10", false);
    }

    #[test]
    fn resolve_of_undisputed_transaction_is_ignored() {
        let mut record = apply(&["deposit, 1, 1, 10"]);
        assert!(record.update(&tx("resolve, 1, 1,")).is_ok());
        assert_account(&record, "10", "0", "10", false);
    }

    #[test]
    fn resolve_of_unknown_transaction_is_ignored() {
        let mut record = apply(&["deposit, 1, 1, 10", "dispute, 1, 1,"]);
        assert!(record.update(&tx("resolve, 1, 99,")).is_ok());
        assert_account(&record, "0", "10", "10", false);
    }

    #[test]
    fn resolved_transaction_cannot_be_disputed_again() {
        let mut record = apply(&["deposit, 1, 1, 10", "dispute, 1, 1,", "resolve, 1, 1,"]);
        assert!(record.update(&tx("dispute, 1, 1,")).is_ok());
        assert_account(&record, "10", "0", "10", false);
    }

    #[test]
    fn chargeback_removes_held_funds_and_locks() {
        let record = apply(&[
            "deposit, 1, 1, 10",
            "deposit, 1, 2, 5",
            "dispute, 1, 1,",
            "chargeback, 1, 1,",
        ]);
        assert_account(&record, "5", "0", "5", true);
    }

    #[test]
    fn chargeback_of_undisputed_transaction_is_ignored() {
        let mut record = apply(&["deposit, 1, 1, 10"]);
        assert!(record.update(&tx("chargeback, 1, 1,")).is_ok());
        assert_account(&record, "10", "0", "10", false);
    }

    #[test]
    fn locked_account_rejects_deposits_and_withdrawals() {
        let mut record = apply(&[
            "deposit, 1, 1, 10",
            "deposit, 1, 2, 5",
            "dispute, 1, 1,",
            "chargeback, 1, 1,",
        ]);
        let deposit = record.update(&tx("deposit, 1, 3, 100"));
        let withdrawal = record.update(&tx("withdrawal, 1, 4, 1"));
        assert!(matches!(deposit, Err(UpdateError::AccountFrozen(..))));
        assert!(matches!(withdrawal, Err(UpdateError::AccountFrozen(..))));
        assert_account(&record, "5", "0", "5", true);
    }

    #[test]
    fn chargeback_after_withdrawal_leaves_negative_total() {
        let record = apply(&[
            "deposit, 1, 1, 10",
            "withdrawal, 1, 2, 10",
            "dispute, 1, 1,",
            "chargeback, 1, 1,",
        ]);
        assert_account(&record, "-10", "0", "-10", true);
    }

    #[test]
    fn unknown_type_is_rejected() {
        let mut record = new_record();
        let result = record.update(&tx("transfer, 1, 1, 10"));
        assert!(matches!(
            result,
            Err(UpdateError::UnknownTransactionType(_))
        ));
        assert_account(&record, "0", "0", "0", false);
    }

    #[test]
    fn overflowing_deposit_is_rejected_and_not_disputable() {
        let max = Decimal::MAX.to_string();
        let mut record = apply(&[&format!("deposit, 1, 1, {max}")]);
        let result = record.update(&tx("deposit, 1, 2, 1"));
        assert!(matches!(result, Err(UpdateError::Overflow(_))));
        assert!(record.update(&tx("dispute, 1, 2,")).is_ok());
        assert_account(&record, &max, "0", &max, false);
    }

    #[test]
    fn rejected_rows_change_nothing() {
        let cases: &[(&[&str], &str)] = &[
            (&[], "deposit, 1, 1,"),
            (&[], "deposit, 1, 1, -1"),
            (&[], "transfer, 1, 1, 5"),
            (&["deposit, 1, 1, 10"], "withdrawal, 1, 2, 20"),
            (
                &["deposit, 1, 1, 10", "withdrawal, 1, 2, 1"],
                "dispute, 1, 2,",
            ),
            (&["deposit, 1, 1, 10", "dispute, 1, 1,"], "dispute, 1, 1,"),
            (
                &["deposit, 1, 1, 10", "dispute, 1, 1,", "chargeback, 1, 1,"],
                "deposit, 1, 2, 5",
            ),
            // `available` fits, but `available + held` overflows.
            (
                &[
                    "deposit, 1, 1, 50000000000000000000000000000",
                    "dispute, 1, 1,",
                ],
                "deposit, 1, 2, 50000000000000000000000000000",
            ),
        ];
        for (setup, row) in cases {
            let mut record = apply(setup);
            let before = record.clone();
            assert!(record.update(&tx(row)).is_err(), "`{row}` is rejected");
            assert_eq!(record, before, "`{row}` changes nothing");
        }
    }

    #[test]
    fn total_is_always_available_plus_held() {
        let mut record = new_record();
        for row in [
            "deposit, 1, 1, 10",
            "deposit, 1, 2, 2.5",
            "withdrawal, 1, 3, 4",
            "dispute, 1, 2,",
            "withdrawal, 1, 4, 100",
            "resolve, 1, 2,",
            "dispute, 1, 1,",
            "chargeback, 1, 1,",
        ] {
            let _ = record.update(&tx(row));
            assert_eq!(
                record.total,
                record.available + record.held,
                "after `{row}`"
            );
        }
    }

    #[test]
    fn limitation_locked_account_rejects_resolve() {
        let mut record = apply(&[
            "deposit, 1, 1, 10",
            "deposit, 1, 2, 5",
            "dispute, 1, 1,",
            "dispute, 1, 2,",
            "chargeback, 1, 1,",
        ]);
        let result = record.update(&tx("resolve, 1, 2,"));
        assert!(matches!(result, Err(UpdateError::AccountFrozen(..))));
        assert_account(&record, "0", "5", "5", true);
    }

    #[test]
    fn limitation_zero_amounts_accepted() {
        let mut record = new_record();
        assert!(record.update(&tx("deposit, 1, 1, 0")).is_ok());
        assert!(record.update(&tx("withdrawal, 1, 2, 0")).is_ok());
        assert_account(&record, "0", "0", "0", false);
    }

    #[test]
    fn stores_only_what_disputes_can_use() {
        let record = apply(&[
            "deposit, 1, 1, 10",
            "deposit, 1, 2, 5",
            "withdrawal, 1, 3, 1",
            "withdrawal, 1, 4, 100",
            "dispute, 1, 1,",
            "resolve, 1, 1,",
            "dispute, 1, 2,",
            "transfer, 1, 5, 1",
        ]);
        let id = |n: u32| tx(&format!("dispute, 1, {n},")).tx;
        assert_eq!(record.deposits, HashMap::from([(id(2), dec("5"))]));
        assert_eq!(record.withdrawals, HashSet::from([id(3)]));
        assert_eq!(record.disputed, HashSet::from([id(2)]));
    }

    #[test]
    fn chargeback_frees_stored_transactions() {
        let record = apply(&[
            "deposit, 1, 1, 10",
            "deposit, 1, 2, 5",
            "withdrawal, 1, 3, 1",
            "dispute, 1, 1,",
            "dispute, 1, 2,",
            "chargeback, 1, 1,",
        ]);
        assert!(record.deposits.is_empty());
        assert!(record.withdrawals.is_empty());
        assert!(record.disputed.is_empty());
    }

    #[test]
    fn format_amount_pads_to_four_places() {
        assert_eq!(format_amount(dec("1.5")), "1.5000");
        assert_eq!(format_amount(dec("2")), "2.0000");
        assert_eq!(format_amount(dec("0")), "0.0000");
    }

    #[test]
    fn format_amount_rounds_half_to_even() {
        assert_eq!(format_amount(dec("1.00005")), "1.0000");
        assert_eq!(format_amount(dec("1.00015")), "1.0002");
        assert_eq!(format_amount(dec("1.123456")), "1.1235");
    }

    #[test]
    fn format_amount_keeps_negative_sign() {
        assert_eq!(format_amount(dec("-10")), "-10.0000");
        assert_eq!(format_amount(dec("-0.00015")), "-0.0002");
    }

    #[test]
    fn format_amount_never_prints_negative_zero() {
        assert_eq!(format_amount(dec("-0.00001")), "0.0000");
        assert_eq!(format_amount(dec("-0")), "0.0000");
    }

    #[test]
    fn format_amount_uses_fewer_places_when_four_do_not_fit() {
        assert_eq!(
            format_amount(dec("10000000000000000000000000")),
            "10000000000000000000000000.000"
        );
        assert_eq!(format_amount(Decimal::MAX), Decimal::MAX.to_string());
        assert_eq!(format_amount(Decimal::MIN), Decimal::MIN.to_string());
    }
}
