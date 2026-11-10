# Security

OpenBMT is pre-release software. It has had internal design reviews only. No
independent security audit has been done. Do not use it with real funds.

## Reporting a problem

Please report security problems privately, through GitHub's "Report a
vulnerability" button on the Security tab of this repository. Do not open a
public issue for a problem that could put funds at risk.

A useful report says what an attacker needs (which device, key or network
position), what they gain, and how to reproduce it. Design-level problems are
as welcome as code bugs: the design is in [docs/DESIGN.md](docs/DESIGN.md)
and its threat table is in section 12.

## What counts

The goal the design is held to is in section 0 of the design document: once
its two launch conditions hold, no single compromised component, device or
person can spend the wallet, and losses from instant acceptance stay within
the limits the owner sets. Anything that breaks that statement, or makes a
screen say more than was actually checked, is a security problem here.
