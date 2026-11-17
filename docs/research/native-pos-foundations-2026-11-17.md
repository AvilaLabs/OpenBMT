# Native POS foundations and connected workflows

17 November 2026. Working source builds on `a79db1a90dfb0f76d72ebfeb983507bb9cbb51d8`. This batch develops the Android test host and Shop Lightning's owner report. It is not a merchant-ready release or a parity claim. Browser demo work is deferred.

The [execution ledger](capability-execution-2026-11-17.md) retains all 502 capabilities and 514 benchmark variants. Seven implementation candidates link 23 capabilities and 24 variants to this work. They remain unverified as complete merchant capabilities; no row closes merely because a model or screen exists.

## Connected native work

| Area | Implemented scope | Remaining boundary |
|---|---|---|
| Priced sale | Immutable USD line/modifier snapshots, quantities, fixed order discount, one noncompounding tax component, fixed tip and exact totals; saved before tender in ledger v7 | No jurisdiction lookup, exemptions, compound taxes, coupon engine, or tax-compliance claim |
| Catalog | Local product revisions, variants, modifier definitions/selections, SKU/barcode identity, search, create/edit/archive and frozen basket selections | Android editor creates one-variant items and edits a selected variant; no modifier-definition editor, CSV catalog import, camera or physical-scanner qualification |
| Stock and handoff | Operator-entered tracked counts or explicit untracked mode; reasoned adjustments; one durable handoff record per sale with stock changes in the same inventory commit | Single device; no physical count proof, purchasing, transfers, multi-location synchronization or authenticated staff authority |
| Cash sessions | Opening float, paid-in/out reasons, net cash from canonical ledger payments, count/close/discrepancy and closed history | Manual accounting; no hardware drawer, deposit workflow, cash refunds, or automatic atomic sale-to-drawer transaction |
| Reports | Device-timezone local date ranges, exact USD totals, separate observed/applied tenders and requested/paid refunds, legacy unknown breakdowns, privacy-confirmed CSV copy/save | Sales-created cohort with current cumulative refund state, not cash flow by event date or accounting compliance; no FX conversion |
| Receipts | Saved pricing components, base item and separate modifier extensions, inclusive versus exclusive tax labels, tender/change and refund evidence | Local text/native rendering; no printer or email delivery qualification |
| Shop Lightning | Counter-sourced advisory confirmed-capacity and second-copy snapshot transported to the owner app | No amount-specific quote, route guarantee, liquidity guarantee or production recovery certification |

Pricing uses checked integer minor units and `BigInteger` intermediates. Fixed discount pennies are allocated proportionally by largest remainder, ties in frozen input order. Tax rounds half-up per line. The model supports inclusive extraction and exclusive addition; the current Android entry form exposes a merchant-entered exclusive flat rate, defaulting to no configured tax. Tip is added afterward. A free line may accompany a positive-total priced basket; a zero-total paid sale remains unsupported.

Ledger v7 validates frozen inputs against their calculated outputs when creating and decoding a priced sale. Versions 2–6 retain their original totals and have no invented pricing breakdown. Catalog edits do not rewrite earlier basket or sale snapshots; a later addition at a different revision becomes a separate frozen line. An exact SKU/barcode lookup selects its matching variant.

## Integrity and recovery

Stock changes occur only on an explicit handoff command, never merely on a payment observation. The inventory store rejects another handoff for the same sale even under a new command ID. A fully settled split tender can qualify; unresolved, unconfirmed, reorganized, late or review-required payment cannot. `withStableSale` holds the ledger version steady during the local inventory commit and blocks reentrant ledger mutations. Future evidence can still change review state after a handoff; it does not automatically restock goods or issue a refund.

Catalog, cash-session and inventory stores use bounded versioned records and atomic writes. Uncertain writes poison the affected instance until reopen and inspection. Optional module startup faults do not erase records or disable the independent payment ledger/manual checkout. Drawer reconciliation links a canonical cash payment once; it does not collect money again. The cash-session store remains separate from the sale ledger, so an interrupted link requires reconciliation.

CSV exports use a reviewed snapshot, escape fields and guard spreadsheet-formula prefixes. A lost document-picker snapshot reports an interrupted export and requires fresh review; it does not silently export newer data. Legacy pricing totals are included in sales totals while unknown component amounts are excluded from the component subtotal and counted explicitly.

## Lightning report compatibility

Report v3 carries source time, chain facts, confirmation depth, estimated confirmed capacity, pending/unknown channel count and second-copy/queued-write facts. Its backup state is derived from the same captured counters serialized in that report. Versions 1 and 2 still read with no readiness snapshot. Missing, stale or future-dated snapshots remain unknown on the owner page; the phone cannot independently verify the counter clock.

Upgrade the owner app before the counter: older owner builds reject report v3. This change does not alter invoice, fee, spending or signing policy. A capacity estimate and a caught-up snapshot do not establish an amount-specific route or independent channel recovery.

Own/starter-wallet first settings reject Shop Lightning under CR-231. The existing nonstarter own-wallet path can enable it later through a wallet-authorized settings change, with the pinned public descriptor and signed address range available for sweeps. A plain Key-app fixture without that wallet source is insufficient. Easy default-wallet onboarding and a Lightning tender inside the standalone checkout remain separate implementation and authority-design gates; the new owner report does not establish either one.

## Validation standing

Local coordinator validation is complete for this bounded batch; hosted CI for the resulting commit must be checked separately.

- All 9 checkout JVM test groups passed after the cash-session v3 link fix. The cash-session v3 codec preserves legacy staff attribution as unknown rather than inventing a value.
- The Rust workspace run passed 6,867 tests with 2 filtered; the same two exhaustive bit-flip tests also passed in an earlier current-code run. An earlier generated-protocol mismatch was corrected by regenerating the protocol artifacts before the passing workspace run.
- The Lightning suite passed 222 tests. Owner bindings JVM tests, including populated and null cross-language golden fixtures, passed. Owner app tests passed: 6,699 app checks, 136 palette checks and 57 feedback checks.
- Checkout and Owner APK builds passed for both ABIs. The cash-audit Checkout rebuild and final workspace Clippy recheck passed. Dependency inventories and licence/source checks passed. The final document-picker navigation fix passed the native build and an observed save-and-return test. Do not infer a fresh CI result for this unpublished batch.

Current emulator evidence preserves the original four checkout sales and records one new priced cash sale. Its frozen arithmetic is subtotal 900, discount 100, tax 66, tip 50, total 916 minor units. A 900 underpayment was refused with ledger bytes unchanged; a 1,000 cash receipt recorded 84 change. A follow-up fix prevents a fully paid cash sale from triggering an unresolved-payment warning. The stock handoff case reduced quantity from 5 to 3, recorded one handoff, and remained single after reopen/restart. A cash session recorded opening float 2,000 plus net linked cash 916 for expected 2,916; a count of 2,900 retained a shortage of 16. The native CSV save picker was verified with two current-date rows: an older 550-minor-unit sale with unknown pricing breakdown and the new 916-minor-unit frozen-pricing sale.

A separate isolated Bitcoin Core 32.1 / LND 0.21.8 harness completed five scenarios with exit code 0 and all assertions passing. Initial JIT zero-confirmation state was refused and not marked paid, with one pending channel and zero confirmed room. After mining one block, chain height 114 showed confirmed room of 290,060,000 msat and no pending channel. The same invoice then succeeded once; the second-copy monitor was observed before the paid report, and paid state survived restart. During Box outage, the report showed the device away and queued writes while checkout was on-chain only; service resumed afterward. A wrong seed and a different counter were refused. Disabling Shop Lightning through signed settings triggered cooperative channel close and eventual seed deletion.

The public descriptor fixture uses mainnet-format `bc1` encoding over strictly Regtest transport. These results do not establish mainnet recovery, sweep-destination correctness, automatic onboarding, production Lightning readiness or a Lightning tender in native checkout. The owner readiness report remains separate from checkout payment integration.

Catalog and inventory corruption were tested independently: the affected module blocked without resetting its file, manual checkout remained available, and the canonical ledger was byte-identical. Both optional stores were restored byte-exact after the synthetic fault. The final APK update preserved all four stores byte for byte. The code-collision test displayed separate labeled SKU and barcode choices. After CSV save on the final APK, navigation returned to the report and its exported bytes matched the reviewed CSV.

See the coordinator's [machine-readable evidence record](../../tools/merchant-acceptance-investigation/evidence/native-pos-foundations-2026-11-17.json) for exact commands and observations. The separate prior commit's 12 CI and 19 end-to-end jobs passed; inspect the workflows for the new published commit rather than attributing those earlier results to this batch. The seven implementation-candidate groups in the execution ledger remain unverified as complete merchant capabilities.

## Release gates still open

The checkout is still explicitly Regtest/test-only. Staff labels are not authentication; [staff authority integration](standalone-staff-authority-2026-11-17.md) remains unmet. Physical hardware, provider eligibility, production card operation, independent wallet/channel recovery, actual merchant shifts, and mainnet/real-funds gates remain open. No new wallet signing permission, custody, publisher ledger or mandatory service is introduced by this work.
