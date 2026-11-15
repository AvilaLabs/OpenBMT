// What a customer's wallet reads from the code on the counter: the payment request of BIP 21, as the counter's
// model writes it (`bitcoin:ADDRESS?amount=0.00010877`, in upper or lower case). The wallet keeps the address and
// the amount in satoshis; it reads nothing else.
"use strict";
(function (root) {
  /** Satoshis in a decimal amount of bitcoin written as text ("0.00010877"), with no floating point; null if it is not one. */
  function satsOf(amount) {
    const m = /^(\d{1,8})(?:\.(\d{1,8}))?$/.exec(amount);
    if (!m) return null;
    return Number(m[1]) * 100000000 + Number((m[2] || "").padEnd(8, "0"));
  }

  /** {address, sats} of a payment request, or null when the text is not one with an address and an amount. */
  function parse(text) {
    const m = /^bitcoin:([a-z0-9]+)\?(.*)$/i.exec(String(text).trim());
    if (!m) return null;
    const amount = m[2].split("&").map((p) => p.split("=")).find((p) => p[0].toLowerCase() === "amount");
    if (!amount) return null;
    const sats = satsOf(amount[1]);
    if (sats === null || sats <= 0) return null;
    // Bech32 addresses are read in either case; the wallet shows them in lower case.
    return { address: m[1].toLowerCase(), sats };
  }

  const api = { parse, satsOf };
  root.OpenBmtPaymentCode = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
