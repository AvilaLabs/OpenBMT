# OpenBMT Box

The Box is the small server of a shop that takes bitcoin with OpenBMT. The
shop's devices (the counters, the owner's phone and the Key app) pass sealed
messages through it. This page covers what the package gives you and how to
pair the Box.

## Documentation

- [OpenBMT repository](https://github.com/AvilaLabs/OpenBMT) - the source,
  the design and the rules the Box follows.
- [Box rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/BOX-RULES.md)
  - what the Box keeps, how it is paired and what it refuses.

## What you get on StartOS

- **A relay for the shop's devices.** The Box has one address, under
  **Interfaces**. It is not a web page: you give the address to the devices,
  you do not open it in a browser.
- **One action, Enter invitation.** It gives the Box the invitation that the
  Key app made, so that the Key app can pair it.
- **Backups.** The Box's whole data (its identity key, its settings, its
  members and mailboxes, and any POS token it was given) is in one volume,
  and a StartOS backup holds all of it.

## Getting set up

1. Start the service and wait until its health check says the Box is
   listening.
2. Open **Interfaces** and copy a LAN address of the Box. It looks like
   `192.168.1.20:8986` or `openbmt-box.local:8986` (the host and port are
   shown there).
3. In the Key app, make an invitation. It shows 30 characters in six groups
   of five and keeps it open for 30 minutes.
4. In the Box's **Actions**, run **Enter invitation** and type the 30
   characters. The Box restarts and holds the invitation, also for 30
   minutes.
5. In the owner's phone, type the Box's address from step 2. The phone
   fetches the Box's claim and carries it to the Key app. The Key app shows
   "Box found" with four words; they must be the four fingerprint words in
   the service log. Approve the Box in the Key app.

If the 30 minutes pass, make a new invitation in the Key app and run the
action again.

## Using it

The Box needs nothing from you once it is paired. The service log shows its
fingerprint words and its state at each start.

## Limitations

- The Box is an early release. It has had no independent security audit, and
  the OpenBMT project says not to use it with real funds yet.
- Whoever has the Box's data has its identity key and any POS token. Keep
  backups as you would keep a password file.
- The Box has no address on Tor of its own. Tor is not part of StartOS: if
  you install the Tor service and add an onion address to the Box's
  interface, devices can reach the Box that way.
- When a point-of-sale connector is set up, the Box reads the POS vendor over
  HTTPS from this server. That outbound connection shows this server's
  network address to the vendor, unless you send it through a VPN or a
  gateway you have set up for that.
- Only the invitation is given from StartOS. Setting up a POS connector
  (its vendor and token) is not an action in this package yet.
