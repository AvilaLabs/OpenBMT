# Rate rules: where the price comes from

Status: specification for `crates/openbmt-rates`, written before the code.
Each rule has an ID so that a test can name the rule it proves. The rules
implement `docs/DESIGN.md` §2.3 (the price is locked per sale and its source
recorded) and the "Rate source manipulated" row of its threat table: two or
more sources, a deviation check, a fixed schedule, fetched over Tor.

The counter prices a sale with a `Rate` and a `RateSource` that its caller
gives it (COUNTER-RULES CR-05, CR-57, CR-58). It does not fetch a price and
does not check one. This library is what stands between the sources and that
call. The app does the fetching, with the platform's own HTTPS through Tor,
so no TLS code is in Rust. Everything that reads an untrusted reply and
decides what it means is here, in memory-safe code with no dependency on a
JSON library, and is fuzzed.

The library has no network, no clock, no storage and no screen. It reads
bytes and a time the caller gives it and returns a price or a refusal.

## 1. The sources

- **RT-01.** The sources are a fixed table compiled into the program, a
  table for each currency. Each source has an id, a letter, an HTTPS
  address, a currency and a shape. Nothing adds, removes or changes a source
  while the program runs. A currency is in the table only where at least two
  independent exchanges quote bitcoin in it directly. Coinbase's spot price
  is used only for the four currencies that have a Coinbase market (USD,
  EUR, GBP, INR). Each source of a currency added on 2026-11-12 traded
  bitcoin in that currency within the 24 hours before the check. Swiss
  francs, Polish zloty and the other currencies not below are not in the
  table: no second exchange was found, or the second one had not traded for
  weeks. The table, as checked on 2026-11-11 and, where a row says so, on
  2026-11-12 (the fixtures of RT-15 for the four dollar sources and for the
  sources of 2026-11-12 are real replies of these addresses; the others are
  made from the exchanges' documented shapes and are marked so in
  `tests/fixtures.rs`):

  | currency | sources (id: shape) |
  |---|---|
  | USD | `kraken`: Kraken, `coinbase`: Coinbase, `bitstamp`: Last, `gemini`: Last |
  | EUR | `kraken-eur`, `coinbase-eur`, `bitstamp-eur`, `gemini-eur` |
  | GBP | `kraken-gbp`, `coinbase-gbp`, `bitstamp-gbp`, `gemini-gbp` |
  | JPY | `kraken-jpy`, `bitflyer-jpy` (a number) |
  | AUD | `kraken-aud`, `luno-aud`, `btcmarkets-aud` |
  | INR | `coinbase-inr`, `wazirx-inr` |
  | KRW | `upbit-krw` (a number), `bithumb-krw` |
  | IDR | `luno-idr`, `indodax-idr` |
  | SGD | `gemini-sgd`, `independentreserve-sgd` (a number) |
  | CAD (checked 2026-11-12) | `kraken-cad`, `ndax-cad` (a number) |
  | MXN (checked 2026-11-12) | `bitso-mxn`, `binance-mxn` |
  | BRL (checked 2026-11-12) | `foxbit-brl`, `okx-brl`, `binance-brl` |
  | TRY (checked 2026-11-12) | `btcturk-try` (a number), `okx-try`, `binance-try` |
  | THB (checked 2026-11-12) | `bitkub-thb`, `binance-thb` |
  | ZAR (checked 2026-11-12) | `luno-zar`, `valr-zar` |

  The addresses are those of the table in `source.rs`; each names its
  currency (RT-17). The dollar sources are:

  | id | address | shape |
  |---|---|---|
  | `kraken` | `https://api.kraken.com/0/public/Ticker?pair=XBTUSD` | Kraken |
  | `coinbase` | `https://api.coinbase.com/v2/prices/BTC-USD/spot` | Coinbase |
  | `bitstamp` | `https://www.bitstamp.net/api/v2/ticker/btcusd/` | Last |
  | `gemini` | `https://api.gemini.com/v1/pubticker/btcusd` | Last |

  Bitcoin Standard (prices in satoshis, CR-193) has no table: no source is
  asked for it.

- **RT-02.** An id is a label under CR-57 (1 to 32 bytes of `a-z`, `0-9`,
  `-`, `+`, `.`). Ids are unique in the table. A letter is unique among the
  sources of one currency, so the label of a combined rate (RT-35) says
  which sources were used. A source that is not for dollars has its currency
  at the end of its id (`kraken-eur`).
- **RT-03.** Each source has a currency, one of the fixed list of CR-193 (USD,
  EUR, GBP, JPY, AUD, INR, KRW, IDR, SGD, CAD, MXN, BRL, TRY, THB, ZAR). A reply
  that names another currency is refused (RT-17), and a set of samples in more
  than one currency is refused (RT-38). The shop's currency is in its signed
  settings (CR-193); the app fetches the sources of that currency only (RT-40).
  An amount is in the minor units of its currency, by ISO 4217: yen and won have
  none (the amount is whole yen or won) and the others have hundredths. Every
  place that read a price in cents reads it in the minor unit of the source's
  currency (RT-22), and each currency has its own band (RT-24).
- **RT-04.** The app's request is plain: a GET, no cookies, no account, no
  key, no user data, and a generic User-Agent. Nothing in a request depends
  on a shop or a sale (RT-41).

## 2. Reading a reply

- **RT-10.** At most 64 KiB (`MAX_RESPONSE_BYTES`) of a reply are read. A
  longer reply is refused from its length, before anything is parsed. The
  limit is passed to the JSON reader of `openbmt-json`, which takes it as a
  parameter (its own default is larger).
- **RT-11.** Arrays and objects may be nested at most 16 deep
  (`MAX_DEPTH`), passed to `openbmt-json` in the same way. A deeper document
  is refused as soon as the 17th level opens, so the depth of the reader's
  own recursion is bounded.
- **RT-12.** A reply is UTF-8, all of it. Any other byte sequence is
  refused before parsing, whether or not it is inside a string.
- **RT-13.** The whole document is parsed with the JSON grammar of RFC 8259,
  not only the fields wanted: a document that is malformed anywhere is
  refused. Numbers follow the grammar (no leading zero, a digit on each side
  of a dot, a full exponent). Strings allow no raw control character and
  every escape is valid; a `\u` surrogate must come in a correct pair. Only
  space, tab, line feed and carriage return are white space. Bytes after the
  document, other than white space, are refused. Literals other than `true`,
  `false` and `null` (`NaN`, `Infinity`) are refused. Nothing is looked up
  until the whole document has been read.
- **RT-14.** A key twice in one object is refused, wherever the object is in
  the document and however the two keys are spelled (an escape and a
  letter are the same key). Only
  the fields of RT-15 are read, so this is stricter than it must be, on
  purpose: a reply that says things twice is never trusted for the one
  thing it says once. The cost is that a source that starts to repeat a key
  somewhere harmless is refused until this rule is looked at again.
- **RT-15.** The price is found by the source's shape, and only there:
  - Kraken: the top-level `error` is an array; `result` is an object with
    exactly one entry; that entry's `c` (last trade) is an array whose first
    item is the price, a string.
  - Coinbase: `data.amount` is the price, a string; `data.currency` is
    checked (RT-17).
  - Last (Bitstamp and Gemini): the top-level `last` is the price, a string.
  - Text: the price is a string at a fixed path of keys and first items
    (Luno `last_trade`, BTC Markets `lastPrice`, WazirX `lastPrice`, Indodax
    `ticker.last`, Bithumb `data.closing_price`, Bitso `payload.last`,
    Binance `price`, OKX the first item of `data`, its `last`, Foxbit the
    first item of `data`, its `last_trade.price`, Bitkub the first item's
    `last`, VALR `lastTradedPrice`).
  - Number: the price is a JSON number at a fixed path (bitFlyer `ltp`,
    Upbit's first item's `trade_price`, Independent Reserve `LastPrice`,
    NDAX `LastTradedPx`, BtcTurk the first item of `data`, its `last`).
    It is read from the number's own digits, which are taken from the same
    bytes after the whole document has been read (RT-13), never as a float.
    A key spelled with an escape is not matched, so such a reply is refused
    as missing the field. The digits then follow RT-20: a number with an
    exponent or a sign is refused.

  A field that is missing is refused, and so is one of another kind. A price
  that is a JSON number where the shape says a string is refused: a number
  would pass through a float in most readers. Where the shape says a number
  (above) a string is refused too. The shapes were checked against the real
  replies of 2026-11-11, and those of the sources added on 2026-11-12
  against their replies of that day, and are as described here, with no
  difference.
  Bitstamp's real reply holds more fields (`bid`, `ask`,
  `timestamp`) and Gemini's holds a `volume` object with its own `USD` key;
  neither is read.
- **RT-16.** Kraken's `error` array that is not empty is a refusal, whatever
  it holds and whatever `result` holds. An `error` that is not an array is
  refused as the wrong shape.
- **RT-17.** The currency of a reply, where the shape has one, must be the
  source's currency exactly (`USD`, not `usd` or `USDT`). Where the shape has
  none (Kraken's pair and the addresses of the others name it) the address
  is the only statement of the currency. NDAX's address names its market by
  a number (`InstrumentId=1`, the bitcoin and Canadian dollar market in
  NDAX's documentation), and its reply holds only that number, so for that
  source the number in the address is the only statement. A shape that has
  a field naming the market (Luno `pair`, BTC Markets `marketId`, WazirX
  `symbol`, bitFlyer `product_code`, Upbit `market`, Independent Reserve
  `SecondaryCurrencyCode`, Bitso `payload.book`, Binance `symbol`, OKX
  `instId`, Foxbit `market_symbol`, BtcTurk `pair`, Bitkub `symbol`, VALR
  `currencyPair`) checks it against the market of the address, and a
  reply for another market is refused as `WrongCurrency`. Bithumb's `status`
  must be `0000`, and anything else is refused as `SourceError`.

## 3. The text of a price

- **RT-20.** A price is the text of a decimal number: ASCII digits and at
  most one dot, with at least one digit before the dot and, if there is a
  dot, at least one after it. No sign, no leading `+`, no exponent, no
  white space, no separator, no digit from another script. A whole part of
  two or more digits does not start with `0`.
- **RT-21.** At most 8 digits after the dot (`MAX_DECIMALS`). The real
  sources give up to 5.
- **RT-22.** The price is worked out exactly in integers, with no floating
  point, and rounded **half up** to a whole minor unit of the source's
  currency: for dollars 73552.175 is 7355218 cents, 73552.174 is 7355217;
  for yen 11868632.5 is 11868633. Truncating would always lean down by up to
  a minor unit a bitcoin; half up has no lean. At about 7 million cents the
  difference between the two is under one part in 7 million.
- **RT-23.** Zero is refused, and so is a price that rounds to zero minor
  units.
- **RT-24.** A price outside the band of its currency is refused **[to
  tune]**. The band of the dollar is 100 USD to 100,000,000 USD per bitcoin
  (10,000 to 10,000,000,000 cents, `MIN_CENTS_PER_BTC`, `MAX_CENTS_PER_BTC`);
  the band of another currency is the dollar band, as minor units, times a
  fixed factor:

  | currency | factor | lowest | highest (minor units per bitcoin) |
  |---|---|---|---|
  | USD, EUR, GBP, AUD, SGD, CAD, MXN, BRL, TRY, THB, ZAR | 1 | 10,000 | 10,000,000,000 |
  | JPY, INR | 100 | 1,000,000 | 1,000,000,000,000 |
  | KRW | 1,000 | 10,000,000 | 10,000,000,000,000 |
  | IDR | 10,000 | 100,000,000 | 100,000,000,000,000 |

  The band is a sanity check against a unit mistake or a broken feed, not a
  forecast; its use is that a price far outside it never reaches the median.
  A number with so many digits that it overflows is outside the band and is
  refused without an overflow. A sample is made against the band of its
  source's currency.

## 4. Combining prices

A `Sample` is a source, the time the app got the reply (Unix seconds) and a
price in minor units per bitcoin in the band of RT-24: a sample outside it
cannot be made.

- **RT-30.** Only samples that CR-58 allows are used: fetched at most 600
  seconds before the time of the call and at most 60 seconds after it. The
  library asks the counter's own `RateSource::is_fresh_at`, so the two
  cannot drift apart. A sample that is not fresh is dropped without a word
  and is not looked at again, so a stale price never causes a
  disagreement (RT-33).
- **RT-31.** At least 2 distinct sources must have a fresh sample
  (`MIN_SOURCES`). Of one source, only the newest fresh sample is used: two
  fetches of one source are one source. Fewer is refused as
  `NotEnoughSources`.
- **RT-32.** The rate is the median of the prices used. With an even count
  it is the **lower** of the two middle prices: with two sources, the lower
  price. The lower is chosen because a lower rate asks the customer for
  slightly more satoshis (CR-08), so when two sources differ the shop is
  never the one that is short. The choice is made once, for every even
  count, so that it is the same everywhere.
- **RT-33.** Every price used must be within 2% of the median
  (`MAX_SPREAD_BPS` = 200 hundredths of a percent) **[to tune]**: for every
  price `p`, `|p - median| * 10,000 <= median * 200`, worked out in 128-bit
  integers. If any price is further, the call is refused as
  `PricesDisagree`, with the lowest and the highest price in the refusal.
  With two sources the one that is not the median is the higher one, so the
  limit is 2% of the lower.
- **RT-34.** No outlier is dropped. With 3 or more sources, one price that
  is far from the others could be set aside and the rest used, and that is
  not done. A source that is far off is either broken or is being
  manipulated, and in both cases the safe answer is that the price is not
  trusted right now: the counter does not offer Bitcoin, the cashier takes
  another kind of payment, and the disagreement is shown to the person
  (RT-50). Dropping an outlier would also let an attacker who can move one
  source move the median by choosing how far to move it. Refusing costs
  availability only, and the next fetch usually agrees again.
- **RT-35.** The `RateSource` label of a rate is `med.` followed by the letter
  of each source used, in the order of the table: `med.kc`, `med.kbg`,
  `med.kcbg`. It is a label under CR-57, at most 4 bytes plus one per source of
  the currency (8 today, for the four sources of USD, EUR and GBP), so it always
  fits the 32 bytes. A test builds the label for every subset of the table.
  Letters are unique (RT-02), so the label says which sources were used.
- **RT-36.** The time of the `RateSource` is the oldest `fetched_at` of the
  samples used. Every one of them is fresh under RT-30, so the oldest is,
  and the counter's own check of CR-58 passes for a rate made at the same
  time.
- **RT-37.** When samples were given and none is fresh the refusal is
  `TooOld`; when none were given, or some are fresh and fewer than 2
  sources have one, it is `NotEnoughSources`.
- **RT-38.** The sources used must have one currency. Otherwise the call is
  refused as `MixedCurrencies` and nothing is combined.

## 5. The schedule (a rule for the app)

The library does not fetch. The app follows these rules. The numbers are in
the library as constants so that the app and the tests read the same ones.

- **RT-40.** Every source of the shop's currency is fetched on a fixed
  interval, `FETCH_INTERVAL_SECS` = 60 seconds **[to tune]**, the same for
  every shop and every source, whether or not anything is for sale. The
  sources of other currencies are not asked, and a shop whose currency is
  Bitcoin Standard asks none. An interval of 60
  seconds keeps several samples inside the 600 seconds of CR-58 and is light
  on the sources.
- **RT-41.** Nothing is fetched because a sale opened, a quote ran out or a
  cashier pressed a key. A request made when a sale opens would tell the
  source when sales happen. The price a sale uses is the newest result of
  `combine` on the samples the schedule has already collected.
- **RT-42.** A fetch goes through Tor when Tor is switched on in the app.
  With Tor off the owner has chosen to let the sources see the shop's
  address, and the app says so where it is chosen.
- **RT-43.** A fetch has a timeout, `FETCH_TIMEOUT_SECS` = 20 seconds
  **[to tune]**, shorter than the interval so that two fetches of one source
  never overlap.
- **RT-44.** A failed fetch, a timeout, a status other than 200 and a reply
  that is refused under RT-10 to RT-24 are each a missing sample. Nothing is
  retried before the next turn and nothing is kept from a refused reply.
  The refusal is kept for the owner as a note (RT-50).
- **RT-45.** The app stamps a sample with its own clock when the whole reply
  has arrived and has been read, and makes a sample only from a reply that
  `parse_response` accepted. A time given by a source is never used. The
  price a sale is quoted at is locked by the counter's quote (CR-05); the
  library locks nothing.

## 6. What a person is told

- **RT-50.** Every refusal has `Words` (SC-01). A refusal of the rate
  (`TooOld`, `NotEnoughSources`, `PricesDisagree`, `MixedCurrencies`) is
  shown to the cashier on the counter screen: it begins "The price is not
  available right now", says to take another kind of payment, and names
  who to tell where the cashier can do nothing else. A refusal of one
  source's reply is a note for the owner; it says the app will ask again.
  The numbers of the prices and the rule number are in the details only.
- **RT-51.** The words are listed in `docs/screens/rates.md`, made by the
  test `sc_12_the_list_is_on_disk`, and pass `check_distinct` (SC-12). A
  second test fails when a refusal is missing from the list.

## 7. Tests

Tests are named after the rule they prove (`rt_22_...`). The real replies of
the four dollar sources, fetched on 2026-11-11, those of the sources added
on 2026-11-12, fetched that day, and replies made from the
documented shapes of the other sources (marked so in the tests) are in
`crates/openbmt-rates/tests/fixtures/`; each is read to the minor unit
written by hand from the file. The fuzz target `rates_parse` feeds a source and
arbitrary bytes to `parse_response`.

## 8. What these rules do not do

- **No fetching.** The library has no network and no TLS. Whether the
  request went through Tor, was answered by the real source, or was changed
  on the way is for the app and the platform.
- **No protection from every source being wrong together.** Four exchanges
  that all report a wrong price, or one party in the path of all of them,
  gives a wrong rate that agrees with itself. The deviation check finds a
  source that differs from the others, not all of them.
- **No protection from a price that moves.** A price that really falls or
  rises by more than 2% in a few minutes may be refused, and the counter
  does not offer Bitcoin until the sources agree again. The limit is a
  guess **[to tune]**.
- **No check of the number against the world.** The band is a sanity check
  and the median is not a market price. Fees, spread and the exchange's
  own last trade are what the sources report.
- **No counter in another currency than the shop's.** A shop has one
  currency (CR-193); a counter that prices in another is not here.
- **No history.** Nothing keeps the samples after a call. The record of a
  quote holds the label and the time, not the sources' own prices.
- **No dropping of a bad source.** A source that is always refused stays in
  the table; taking it out is a change to the program.
- **No clock.** The time comes from the caller. A wrong clock makes fresh
  samples look stale or the reverse, as CR-58 already says.
- **No claim that the sources will keep their format.** The replies were
  read on one day. A change of shape is a refusal, shown as a note to the
  owner, and a change to the table.
