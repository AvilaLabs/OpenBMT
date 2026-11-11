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

- **A relay for the shop's devices.** The Box's address, under
  **Interfaces**, is not a web page: you give it to the devices, you do not
  open it in a browser.
- **A status page.** The **Status page** interface opens in a browser. It
  says whether the Box is waiting for an invitation, has one open, is paired
  or is turned off, what that means and what to do next, and while the Box
  waits it has a box to type the invitation in.
- **One action, Enter invitation.** It gives the Box the invitation that the
  Key app made, as the status page does, so that the Key app can pair it.
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
4. Open the **Status page** and type the 30 characters in its box, or, in
   the Box's **Actions**, run **Enter invitation** and type them there (the
   Box then restarts). The Box holds the invitation for 30 minutes.
5. In the owner's phone, type the Box's address from step 2. The phone
   fetches the Box's claim and carries it to the Key app. The Key app shows
   "Box found" with four words; they must be the four fingerprint words in
   the service log. Approve the Box in the Key app.

If the 30 minutes pass, make a new invitation in the Key app and enter it
again.

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
- The status page shows no key, invitation or token, and takes only the
  invitation, and only while the Box waits for one.
- Only the invitation is given from StartOS. Setting up a POS connector
  (its vendor and token) is not an action in this package yet.
