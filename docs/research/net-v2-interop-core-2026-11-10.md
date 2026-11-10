# The encrypted transport against Bitcoin Core (2026-11-10)

**Question.** Does the counter's peer session (`openbmt-net`) and network
driver (`openbmt-net-io`) speak the BIP324 encrypted transport with Bitcoin
Core, derive the same session ID as Core, and fall back to the old
transport once, and only once, against a node that does not speak it?

**Answer.** Yes, in these runs. Against a node with `-v2transport=1` every
step ran over the encrypted transport, and the session ID printed by the
node and by the example was the same. Against a node with
`-v2transport=0` the first connection fell back to the old transport, and
the next connection went straight to it. Every step of both scripts passed
in both settings, in two full runs.

## Setup

- Node: `Bitcoin Core daemon version v32.1.0`, taken from
  `~/.local/opt/bitcoin-core-32.1/bin` (the scripts check that `bitcoind`
  is that file). Regtest on 127.0.0.1, a fresh scratch directory under
  `~/.cache/openbmt/`, removed on every exit. Nothing ran against mainnet
  or signet, and `~/.bitcoin` was not touched.
- Session check: `scripts/net-interop-core.sh`, with the example
  `crates/openbmt-net/examples/regtest_peer.rs`. The example offers the
  encrypted transport first. After a peer that closes before any packet
  authenticated it connects again with the old one and remembers that port
  for a day in a file (`REGTEST_MEMORY`), as the counter's address book
  does. After the handshake each command prints `transport`, `fallback` and
  `session_id`.
- Driver check: `scripts/netio-interop-core.sh`, with the example
  `crates/openbmt-net-io/examples/regtest_node.rs`. Its `peers` line ends
  in `v2=<n>`, the number of ready peers on the encrypted transport.
- `scripts/net-v2-interop-core.sh` runs the two scripts with
  `DRILL_V2TRANSPORT=1` and then `DRILL_V2TRANSPORT=0`, which become the
  node's `-v2transport` setting.
- `scripts/counter-e2e-core.sh` was run too. It does not state the
  transport, so it is not counted as a v2 check (see the end).

## Session, `net-interop-core.sh`

| Step | v2transport=1 | v2transport=0 |
| --- | --- | --- |
| 1. Node, 2,105 blocks, filter index | PASS | PASS |
| 2. Handshake: services, height, filters | PASS, `transport v2`, `fallback no` | PASS, first connection `fallback yes`, next `fallback no`, both `v1` |
| 2b. `getpeerinfo` transport and session ID | `v2`; IDs equal | `v1`; no ID on either side |
| 3. Header sync, 2,105 headers in 2 messages | PASS | PASS |
| 4. New block announced unasked | PASS (headers) | PASS (headers) |
| 5. Payment announced, fetched, parsed | PASS | PASS |
| 6. Compact filter matches / does not match | PASS | PASS |
| 7. Full block proves the payment | PASS | PASS |
| 8. Silence after the handshake, 15 s | PASS, 0 bytes | PASS, 0 bytes |
| 9. A port that is not a peer (RPC port) | PASS, `dropped NR-14` after 30 s | PASS, same |

Step 2b, from the first run with `-v2transport=1` (the IDs change on every
connection):

```
node: {"transport_protocol_type":"v2","session_id":"8bc1e776e75754e83a58ab460937c518422cb5cc0eb22dee870de2ecd7a519c9"}
ours: transport v2, session_id 8bc1e776e75754e83a58ab460937c518422cb5cc0eb22dee870de2ecd7a519c9
```

With `-v2transport=0` the node reported `{"transport_protocol_type":"v1",
"session_id":""}`.

The services of the node are `0xc49` with v2 (`P2P_V2` is bit 11) and
`0x449` without it. The number is the same as before apart from that bit.

Step 8 on the encrypted transport cannot read the packets, so the example
counts the bytes it wrote after the handshake and does not count a
29-byte write, which is one `pong` (9 bytes of contents and 20 of
overhead). The old transport keeps its message-by-message count.

Step 9 sends the 64 key bytes to Core's RPC (HTTP) port. That port does not
close the connection, the handshake limit of NR-14 ends the session after 30 seconds,
and the line `dropped NR-14: the handshake took 30 seconds or more` is
printed. A timeout is not taken for a peer that does not speak the
encrypted transport, so there is no second try with the old transport. The
step's own time limit is 80 seconds, which leaves room for two handshake
limits.

## Driver, `netio-interop-core.sh`

| Step | v2transport=1 | v2transport=0 |
| --- | --- | --- |
| 1. Node, 150 blocks, one watched script | PASS | PASS |
| 2. First `Peers` event with a ready peer | `v2=1`; node says `v2`; refusals noted: 0 | `v2=0`; node says `v1`; refusals noted: 1 |
| 3. 150 headers as `Chain` events | PASS | PASS |
| 4. Payment seen before it is mined | PASS | PASS |
| 5. The block that holds it | PASS | PASS |
| 6. Reorganization (one block leaves) | PASS | PASS |
| 7. Stop, mine, start from the stored headers | PASS, about 100 bytes of headers | PASS, about 100 bytes |
| 8. Node drops the driver; the next connection | on `v2`; refusals 0 | on `v1`; refusals still 1 |

Step 8 is the NR-81 check at the level of the driver. The node ends the
connection with `disconnectnode`, the driver dials again after its pause
(NR-70), and the new connection is on the transport that worked. In the
`-v2transport=0` run the note `peer ... does not speak the encrypted
transport; trying the old one` was printed once in that run, so the second
connection did not try the encrypted transport again. The memory is the
driver's own and is not saved: a driver that is stopped and started again
tries the encrypted transport once more (the step 7 restart shows this,
with one refusal in each run).

## Which of the test suite ran against what

| Check | What ran |
| --- | --- |
| BIP324 vectors, packet encoding and decoding, rekeying, handshake | The `bip324` crate's own unit tests: 21 tests, among them `test_vector_1` to `test_vector_7`, all passing, run in a copy of the crate outside this workspace with `bitcoin 0.32.8` added as a dev-dependency (its tests use `bitcoin::secp256k1::rand`, which the published crate does not declare). Its integration test `round_trips` needs a sibling crate of the upstream repository and was not run. |
| Our use of the crate | Tests in `crates/openbmt-net/src/tests/v2.rs`: two of our own sessions through `v2.rs` with an in-test responder, bit flips, cuts at every byte boundary, rekey after 224 messages in both directions, decoys, garbage of 0, 1 and 4,095 bytes accepted and 4,096 refused, a v1 `version` as the first bytes (fallback signal), no fallback after the first encrypted packet. |
| Session and manager tests on both transports | The kit is parameterized. 10 session tests and 19 manager tests run again over the encrypted transport. |
| Driver tests on loopback | `crates/openbmt-net-io/src/tests/v2.rs`: a fake encrypted peer, a peer that closes on the key, a peer that answers with a v1 `version`, endless garbage, a SOCKS5 proxy, and a randomness source that may be guessed. |
| Core | The two tables above. |

## What is not covered

- Only Bitcoin Core 32.1 was tried. Other implementations of BIP324 were
  not. The table of short message IDs in `v2.rs` was written from memory of
  the specification, not copied from a file, and Core is its only check.
  Core accepted every message the session sent in these steps (`version`,
  `verack`, `sendaddrv2`, `sendheaders`, `getheaders`, `getdata`,
  `getcfilters`, `pong`) and the session accepted every message it got, so
  an error in the entries those use would have shown. An entry that no step
  sends or receives (for example `cfheaders` or `merkleblock`) is not
  proven by this run.
- No decoy packet and no garbage from Core was seen in these runs. Both
  are covered by the unit tests only.
- Rekeying (every 224 messages) was not reached with Core. The unit tests
  cover it in both directions.
- Tor: the driver test uses a SOCKS5 proxy on loopback. No Tor daemon was
  run.
- The v2 steps do not show what an observer on the path sees. The
  encrypted transport hides the content from a passive observer, but an
  attacker on the path can force the old transport by closing the
  connection after the key (NR-81). No claim of privacy is made beyond
  that.
- `scripts/counter-e2e-core.sh` passed twice with Core at its default
  (`-v2transport=1`, so the driver used the encrypted transport), but the
  script does not read the transport, so it does not prove it.
- Mainnet and signet were not used.
