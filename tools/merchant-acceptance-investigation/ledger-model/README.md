# SQLite ledger model

Run from this directory with `python3 -m unittest -v`. This dependency-free
prototype models local sale/tender routing, retry identifiers, late payment
handling, refund intent, and a sequential inventory adjustment model.

All outcomes come from synthetic adapters or direct test callbacks; amounts are
fiat cents placeholders. It has no card SDK, Bitcoin node, invoice settlement,
wallet, fee, confirmation policy, authentication, webhook verification, payment
dispatcher, provider reconciliation, refund settlement, or accounting-grade
workflow. SQLite WAL state must be retained together with its WAL/SHM files.
This prototype does not establish external exactly-once behavior or production
recovery.
