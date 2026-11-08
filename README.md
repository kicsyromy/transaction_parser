# transaction_parser

A small payments engine. It reads a CSV file of transactions (deposits, withdrawals, disputes,
resolves and chargebacks), applies them in file order to per-client accounts, and writes the final
state of every account to stdout as CSV.

## Building

Requires Rust 1.85 or newer (the crate uses edition 2024).

```sh
cargo build --release
```

## Running

```sh
cargo run --release -- transactions.csv > accounts.csv
```

The input file is the only argument. Accounts are written to stdout. Log messages go to stderr,
colored only when stderr is a terminal; append `2>/dev/null` to hide them.

The exit code is `0` on success and `1` on a fatal error (see [Errors](#errors)).

### Input

A CSV file with the header `type, client, tx, amount`:

| Column   | Format                                                                  |
|----------|-------------------------------------------------------------------------|
| `type`   | `deposit`, `withdrawal`, `dispute`, `resolve` or `chargeback`           |
| `client` | client ID, `u16`                                                        |
| `tx`     | transaction ID, `u32`                                                   |
| `amount` | decimal; required for deposits and withdrawals, empty or omitted otherwise |

Disputes, resolves and chargebacks refer to an earlier deposit through its `tx`.

### Output

A CSV file with the header `client, available, held, total, locked` and one row per client.
Amounts have exactly four decimal places. Row order is unspecified.

### Example

Input:

```csv
type,       client, tx, amount
deposit,    1,      1,  10.0
deposit,    2,      2,  5.25
withdrawal, 1,      3,  2.5
dispute,    2,      2,
chargeback, 2,      2,
deposit,    2,      4,  1.0
```

Output:

```csv
client,available,held,total,locked
1,7.5000,0.0000,7.5000,false
2,0.0000,0.0000,0.0000,true
```

The chargeback reverses client 2's deposit and locks the account, so deposit `4` is rejected.

## Testing

```sh
cargo test
```

- Unit tests sit next to the code: transaction rules and output formatting in
  `src/client_record.rs`, input parsing in `src/transaction.rs`.
- End-to-end tests in `tests/end_to_end.rs` run the compiled binary on the cases in
  `tests/fixtures/<case>/`. Each case holds an `input.csv` and the `expected.csv` output. Rows are
  compared in any order.
- Tests named `limitation_*` pin the behavior listed under
  [Known limitations](#known-limitations) and fail when that behavior changes.

To add an end-to-end case, create `tests/fixtures/<case>/input.csv` and `expected.csv`, then add a
test that calls `check_fixture("<case>")`.

Formatting and lints:

```sh
cargo fmt --check
cargo clippy --all-targets
```

## Code layout

- `src/main.rs`: command line handling, one account per client, writing the output.
- `src/transaction.rs`: types for an input row and the CSV reader settings.
- `src/client_record.rs`: account balances, the rules for each transaction type, and output
  formatting.

## Design decisions

### Input

- Whitespace around headers and fields is trimmed.
- Type names are lowercase. Any other value, including `Deposit`, is an unknown type; the row is
  skipped and logged.
- Transaction IDs are assumed to be unique, as the specification guarantees. Duplicates are not
  detected.
- A client's account is created by the first row for that client that does not fail. If that row
  fails, for example a withdrawal from an empty account, no account is created.

### Amounts

- Amounts use exact decimal arithmetic (`rust_decimal`). No floating point is involved.
- Negative amounts are rejected.
- Amounts with more than four decimal places are kept at full precision. Output is rounded to four
  places with round half to even (banker's rounding): `1.00005` becomes `1.0000` and `1.00015`
  becomes `1.0002`.
- Arithmetic is checked. A row that would overflow a balance is rejected.
- `total` is recomputed as `available + held` after every row, so the three values always agree.

### Deposits and withdrawals

- A withdrawal needs `available >= amount`. Held funds can't be withdrawn.
- A failed withdrawal changes nothing and can't be disputed.

### Disputes, resolves and chargebacks

- Only deposits can be disputed. A dispute holds funds that were credited to the account, and a
  withdrawal's funds have already left it. Disputes of withdrawals are rejected.
- A dispute, resolve or chargeback only matches a transaction of the same client. One that names
  another client's transaction, or a transaction that doesn't exist, is ignored and logged.
- Disputing a transaction that is already under dispute is rejected.
- Resolves and chargebacks of a transaction that isn't under dispute are ignored and logged.
- Once resolved, a transaction can't be disputed again.
- A chargeback is final. It removes the held funds from the account and locks it.

### Negative balances

A dispute can push `available` below zero when the disputed deposit was already withdrawn. A
chargeback then leaves `total` negative:

```csv
type,       client, tx, amount
deposit,    1,      1,  10.0
withdrawal, 1,      2,  10.0
dispute,    1,      1,
chargeback, 1,      1,
```

This gives `1,-10.0000,0.0000,-10.0000,true`. The negative balance is what the client owes after
the deposit is reversed.

### Errors

- A problem with a single row (insufficient funds, unknown type, locked account, a reference to an
  unknown transaction, and so on) is logged to stderr and the row is skipped. Processing continues.
- Fatal errors stop the run with exit code `1`: a missing argument, a file that can't be opened,
  or a row that can't be parsed.

### Memory

Rows are read one at a time and not kept. Each account stores only what a later row can refer to:

- deposits that can still be disputed, with their amounts
- IDs of successful withdrawals, so a dispute of one is reported as such
- the deposits currently under dispute

A resolved or charged-back deposit is removed. A locked account rejects every later row, so
everything it stores is freed.

## Known limitations

- A locked account rejects every later row, including resolves and chargebacks of its other open
  disputes. Funds held by those disputes stay held.
- An ignored dispute, resolve or chargeback counts as a row that didn't fail, so it can create a
  zero-balance account for a client with no other activity.
- A row that can't be parsed (for example client `abc` or `70000`) stops the run.
- Deposits and withdrawals of zero are accepted.
- Balances above about 7.9 × 10^24 are printed with fewer than four decimal places, because the
  decimal type can't hold four places at that size.
- Everything stored stays in memory, with no disk storage. A deposit that can still be disputed
  takes 24 to 48 bytes (2.4 to 4.8 GB per 100 million), and a successful withdrawal 6 to 11
  bytes, depending on how full the hash tables are.
