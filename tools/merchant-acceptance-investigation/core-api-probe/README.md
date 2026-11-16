# Counter core API probe

Run from the repository root with
`cargo run --locked --manifest-path tools/merchant-acceptance-investigation/core-api-probe/Cargo.toml`.
This isolated package exercises the real `openbmt-counter` public API and its
`openbmt-core` and `openbmt-watch` dependencies using synthetic transaction
facts. Its Cargo workspace is isolated from the repository workspace.

The harness covers local sale opening and judging, duplicate sighting behavior,
a till-origin association across an in-memory `Stored` snapshot reload, a paid
report, and late payment after cancellation. Transactions, headers, health, and
sightings are synthetic. The snapshot reload is in memory; this is not a disk
restart, Bitcoin settlement, or merchant fulfillment test. A payment acceptance
report does not acknowledge cashier fulfillment, and no real funds or external
services are used.
