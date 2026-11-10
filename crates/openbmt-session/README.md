# openbmt-session

The approval session file, as a Rust library with no network, no clock, no
storage and no randomness. The owner phone writes a request (a list of items
with the owner's signature), the Key app writes a response (one result per
item, with the approver's signature), and both sides read them with the same
strict decoder. Used by `openbmt-approver` and `openbmt-owner`.

The specification is [docs/SESSION-RULES.md](../../docs/SESSION-RULES.md),
section 1 (AS-01 to AS-15). Every error says which rule it comes from, and
every test is named for the rule it proves.

**This crate has had internal review only. No independent security audit has
been done yet.** Do not use it with real funds.

## What is in it

| Type | What it is | Rules |
|---|---|---|
| `Request`, `RequestItem`, `Item`, `Label` | The file the owner phone hands to the Key app: at most 64 items of five kinds (a spend, a batch awaiting co-signature, settings awaiting co-signature, a pairing, a health check), at most 8 MiB, signed by the owner | AS-01 to AS-11 |
| `Response`, `ItemResult`, `Verdict`, `Payload` | The file the Key app hands back: one result per item, approved with a payload or declined with a code and text, signed by the approver | AS-12 |
| `Response::check_against` | The owner side's binding checks: session, request hash, items, kinds and signature against the pinned approver root | AS-13, AS-14 |

Decoding only parses and checks the shape. A label proves nothing: it is the
owner phone's claim.
