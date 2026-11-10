# openbmt-approver

The Key app's side of the approval session, as a Rust library with no
network, no clock of its own and no screen. It opens the request the owner
phone made, lists every line with what the Key app's own checks say (never
what the owner phone's labels say), takes the person's decision on each line,
signs what was approved through the existing crates, and stores all new state
before the response bytes leave.

The specification is [docs/SESSION-RULES.md](../../docs/SESSION-RULES.md),
section 2 (AS-20 to AS-38). Every refusal says which rule it comes from, and
every test is named for the rule it proves.

**This crate has had internal review only. No independent security audit has
been done yet.** Do not use it with real funds.

## The calls

| Call | What it does | Rules |
|---|---|---|
| `Approver::new`, `attach_signer`, `restore` | The Key app's identity, wallet and pinned owner root; key 2 and its signer state when this device holds key 2; the stored snapshot back | AS-36, AS-38 |
| `Approver::open` | Size, frame, owner signature, wallet ID and session ID, in that order. Stores and releases nothing, and changes nothing the Key app keeps | AS-20 to AS-22 |
| `Session::lines` | One line per item, from the Key app's own checks, and whether it can be approved | AS-23 to AS-29 |
| `Session::decide` | Approve or decline one line | AS-30 to AS-32 |
| `Session::finish` | Signs the approved lines through the signer and the core crate, hands one snapshot to `persist`, and only then returns the response bytes | AS-33 to AS-37 |

## What it is not

It has no screen, no camera, no PIN and no transport. The Key app's own
screen calls (enrolling a destination, setting ceilings, raising a mark) go
through `change_signer` and `raise_receive_mark` so that they are stored with
the rest. The full list of what the rules do not do is in section 4 of the
rules.
