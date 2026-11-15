# Giving rules: OpenBMT for churches and nonprofits

A church, a charity or any place that takes gifts rather than sales uses
the same apps, the same wallet and the same checks as a shop. What changes
is what the screens say, who types the amount, and which shop-only steps
are offered. This file holds the rules of that difference. Funds, the
public view, signed reasons and the printed code follow in later sections.
Tests are named after the rule (`gv_01_...`).

## 1. The kind of place

- **GV-01. A setting of the shop.** The signed settings carry `place`: a
  shop (0) or a place of giving (1). At first it is a shop. It is written
  only for a place of giving, in settings layout 6: the fields of layout 5
  with the Shop Lightning block behind a flag of its own (a place of giving
  may have none), then one byte of the place. Settings of a shop are the
  bytes every earlier build wrote, with the same hash. A frame carries
  layout 6 the way it carries any layout after a layout byte: frame
  version 6 with the owner wallet's signature, 7 with the signatures of
  two cosigners (`docs/APPROVAL-RULES.md` AP-14, AP-17), and 8 with
  neither. Layout 6 whose place byte says shop is refused (it would have
  been written in an earlier layout), and so is any other value of it.
- **GV-02. Its class.** Changing a shop into a place of giving is Loosen
  (`docs/APPROVAL-RULES.md` AP-01): at a place of giving the person paying
  types the amount (GV-11), which a shop must not allow without a counter's
  confirmation. Changing a place of giving into a shop is Free.
- **GV-03. Asked once, at the start.** The owner phone's first run asks
  what kind of place it is, before the wallet: a shop, or a church or
  another place that takes gifts. The answer is written in the first
  settings. It can be changed later in the shop's settings, as GV-02 says.

## 2. What a place of giving changes

- **GV-10. Words.** At a place of giving the counter app and the owner
  app use the words of giving where a shop's words name a sale: a gift for a
  sale, a giver for a customer, an amount for a price, and a giving point
  for a counter. The rules, the checks, the records and the code's names
  stay those of a sale. Words that do not name a sale are the same.
- **GV-11. The giver types the amount.** A giving point opens on the
  amount: the giver types it, in the shop's currency or in bitcoin, and the
  code shows once it is confirmed. The limits of the settings (minimum and
  largest sale) apply to it as to a sale, with the words of giving.
- **GV-12. Nothing to hand over.** The end of a gift says that it has
  arrived and thanks the giver. It never asks anyone to hand something over
  or to hold it back, because nothing is handed over. A gift is counted, in
  the books and on the owner phone, as a sale is: unconfirmed until it
  confirms, and taken back if it does not.
- **GV-13. Shop-only steps.** At a place of giving, the first run and the
  home screens do not offer tips, the POS connector (`docs/POS-RULES.md`),
  the till of the Box or the cashier list. Each stays in the settings, for a
  place that also sells, such as a café or a bookshop, and works there as in
  a shop when it is turned on. Tips stay off until they are.
- **GV-14. Left unattended.** A giving point is often left where people
  pass. It shows only the amount screen and the gift in progress; every
  setting, the gift list and the day's totals need the manager's PIN, and
  the screen returns to the amount after two minutes without a touch.
