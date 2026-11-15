// What a customer's wallet reads from the code on the counter: the payment request of BIP 21, as the counter's
// model writes it (`bitcoin:ADDRESS?amount=0.00010877`, in upper or lower case). The wallet keeps the address and
// the amount in satoshis, and the Lightning invoice when the request carries one (`&lightning=lnbc...`); it reads
// nothing else. A phone that taps reads the same request from the counter's tag: an NDEF file with one URI record
// (docs/TAP-RULES.md TP-01, TP-02), whose text [fromTag] gives back.
"use strict";
(function (root) {
  /** Satoshis in a decimal amount of bitcoin written as text ("0.00010877"), with no floating point; null if it is not one. */
  function satsOf(amount) {
    const m = /^(\d{1,8})(?:\.(\d{1,8}))?$/.exec(amount);
    if (!m) return null;
    return Number(m[1]) * 100000000 + Number((m[2] || "").padEnd(8, "0"));
  }

  /** {address, sats} of a payment request, with {lightning} when it offers an invoice; null when it has no address or amount. */
  function parse(text) {
    const m = /^bitcoin:([a-z0-9]+)\?(.*)$/i.exec(String(text).trim());
    if (!m) return null;
    const amount = m[2].split("&").map((p) => p.split("=")).find((p) => p[0].toLowerCase() === "amount");
    if (!amount) return null;
    const sats = satsOf(amount[1]);
    if (sats === null || sats <= 0) return null;
    // Bech32 addresses are read in either case; the wallet shows them in lower case.
    const read = { address: m[1].toLowerCase(), sats };
    const invoice = m[2].split("&").map((p) => p.split("=")).find((p) => p[0].toLowerCase() === "lightning" && p[1]);
    if (invoice) read.lightning = invoice[1].toLowerCase();
    return read;
  }

  /**
   * The text of the one URI record in a tag's NDEF file (TP-02: two bytes of length, then a plain record of type U
   * with identifier code 0), or null when the file is anything else. [bytes] may be signed, as the model gives them.
   */
  function fromTag(bytes) {
    if (!bytes || bytes.length < 2) return null;
    const b = Array.from(bytes, (x) => x & 0xff);
    const message = b.slice(2);
    if (message.length !== b[0] * 256 + b[1] || message.length < 6) return null;
    const flags = message[0];
    // The first and last record, not chunked, with no identifier, of the well-known type U.
    if ((flags & 0xe8) !== 0xc0 || (flags & 0x07) !== 1 || message[1] !== 1) return null;
    const short = (flags & 0x10) !== 0;
    const at = short ? 3 : 6;
    const len = short ? message[2] : ((message[2] * 256 + message[3]) * 256 + message[4]) * 256 + message[5];
    if (message.length !== at + 1 + len || len < 1 || message[at] !== 0x55 || message[at + 1] !== 0) return null;
    return new TextDecoder().decode(Uint8Array.from(message.slice(at + 2)));
  }

  const api = { parse, satsOf, fromTag };
  root.OpenBmtPaymentCode = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
