# Tap rules: the payment request over NFC

Status: specification, written before the code, for the tag module of
`crates/openbmt-counter-ffi` and the tag service of the counter app. Each
rule has an ID so that a test can name the rule it proves (`tp_NN_...`).
The rules implement `docs/DESIGN.md` §7.

The counter phone acts as an NFC tag. A customer who holds an Android
phone to it gets the same payment request the screen shows as a QR code,
and their wallet opens with the address and amount filled in. The tag
holds nothing the QR code does not. The counter reads nothing from the
customer's phone, and nothing a tap brings in changes a sale.

**First build. No independent security audit has been done. Not for real
funds.**

## 1. What the tag holds

- **TP-01. The request the screen shows.** The tag holds one NDEF message
  with one URI record. The record's text is the sale's request in the form
  the app copies (CR-83), with the Lightning invoice when the sale offers
  one (LN-17), in lower case. It is made by the Rust core from the same
  call that makes the QR code's text, at the same moment. The app passes the
  bytes on and never builds, reads or changes them. A test checks that the
  two are the same text, letter case aside, for every form of request.
- **TP-02. One plain record.** The record is an NFC Forum URI record (TNF
  1, type `U`) with identifier code 0 (no prefix abbreviated). It is a
  short record when its payload is at most 255 bytes. The message has no
  second record and no Android Application Record, so the customer's phone
  picks the wallet, not the counter.
- **TP-03. Size.** The message is at most 2048 bytes. A request that does
  not fit is not put on the tag; the QR code is shown as before and the tag
  stays empty (TP-04).
- **TP-04. Empty when no sale is waiting.** While no sale is waiting for
  payment on the payment screen, the tag answers every command with "not
  found" (status `6A82`) and holds no request, not even the last one.
- **TP-05. Changes with the sale.** When the request changes, because part
  was paid or the sale ended, the tag's content changes when the QR code
  does. A reader gets the content as it was when it selected the NDEF file;
  a later selection gets the new content. When the sale ends, reads of the
  old content are refused.
- **TP-06. Nothing more about the shop.** The record holds no label, shop
  name, sale number or counter ID. These are not in the QR code either.

## 2. The tag's commands

- **TP-10. A Type 4 tag, read-only.** The counter answers as an NFC Forum
  Type 4 tag, application ID `D2760000850101`, registered in the category
  `other`. It is not a payment app in Android's sense and never asks to be
  the phone's default for payments.
- **TP-11. Three commands.** The tag answers only:
  - SELECT by name of its application ID;
  - SELECT by file ID of the capability file `E103` or the NDEF file
    `E104`;
  - READ BINARY of the file selected.

  Any other instruction is refused with `6D00`, any other class byte with
  `6E00`. UPDATE BINARY is refused with `6982`: no reader can write to the
  tag.
- **TP-12. The capability file.** It says mapping version 2.0, at most 255
  bytes per read, NDEF file `E104` of at most 2050 bytes, read access `00`
  and write access `FF`.
- **TP-13. Reads in order.** A READ BINARY before a file is selected, or
  after the application is selected again, is refused with `6986`. A read
  that starts beyond the file is refused with `6B00`. A read that runs past
  the end answers the bytes up to the end. A read never answers more than
  255 bytes.
- **TP-14. Parsed by the Rust core.** Each command reaches the Rust core as
  bytes and is parsed there with every length checked. A command longer
  than 261 bytes, or whose lengths do not agree, is refused with `6700` and
  changes nothing. The parser has a fuzz target.
- **TP-15. Forgotten at the end of the tap.** When the reader goes away,
  the file selection and the content it read are dropped.

## 3. When the tag answers

- **TP-20. Only on the payment screen.** The counter app asks Android to
  send its commands to the counter only while the payment screen of a sale
  is in front, and stops when that screen leaves. At other times the
  counter answers nothing (TP-04).
- **TP-21. Screen on and unlocked.** The tag answers only while the counter
  phone's screen is on and unlocked. The service asks Android for this where
  the Android version supports it, and the app checks both itself on every
  command as well.
- **TP-22. The counter app only.** Only the counter app has the tag
  service. The owner app, the Key app and the Box have none.
- **TP-23. No tag reading.** The counter app registers for no NFC
  discovery, takes no NFC intents and does not read tags. Bolt Card and
  other customer cards are not part of this (section 5).

## 4. What a tap does not change

- **TP-30. The same checks as a scan.** A tap gives the customer's wallet
  the same request as the QR code. The customer's wallet shows the address
  and amount before it signs, and the counter screen shows the amount and
  the start and end of the address to check them against
  (`docs/DESIGN.md` §2.3). A sale is paid only by what arrives on the chain
  or over Lightning, as for any other sale.
- **TP-31. Another phone in the queue.** A phone held between the
  customer and the counter can pretend to be the counter's tag and give a
  different request. The customer checks the amount and address on their
  wallet against the counter screen, as for a sticker over the QR code
  (`docs/DESIGN.md` §7). The counter's own screen still says "Not paid yet".
- **TP-32. A relay.** Someone who passes the counter's tag on to a phone
  elsewhere gives that phone the shop's own request. Paying it pays the
  shop. The relay learns only what the QR code shows.
- **TP-33. Phones the tag cannot reach.** An iPhone opens no wallet from a
  `bitcoin:` tag, so iPhone customers scan the QR code. An Android wallet
  that has not registered for `bitcoin:` links does not open, and newer
  Android versions may ask the customer before an app opens from a tag.
  Which wallets open is tested on real phones (`docs/DESIGN.md` E1). The QR
  code is always on the screen beside the tap.

## 5. Customer cards: not built

- **TP-40. Why not.** A Bolt Card's money is held by the server that issued
  the card, unless the customer runs that server, and paying with it makes
  the counter fetch a web address the card names. That is custody, and a
  request from the shop to an address any card can choose. Neither is
  built. A later rule set may reopen it with a server the customer runs.

## 6. Tests

- **TP-50.** The tag's bytes for each form of request (on-chain, with a
  Lightning invoice, the longest that fits, one that does not fit) read back
  by a separate NDEF parser in the test and compared with the QR code's
  text (TP-01 to TP-03); every command path of TP-10 to TP-15 with its
  status word; the fuzz target of TP-14; TP-04 and TP-05 across a sale's
  screens; and a check of the built app's manifest and service file: one
  application ID, category `other`, no NFC discovery filters (TP-10,
  TP-23). The real-phone runs of E1 are a separate step.
