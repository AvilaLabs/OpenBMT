# openbmt-rates

The price of bitcoin from several sources, for the counter. The app fetches
the replies itself over Tor with the platform's HTTPS; this library reads
them and decides. The rules are in `docs/RATE-RULES.md` (RT-01 to RT-51).

- `parse_response(source, bytes)` reads one source's reply into cents per
  bitcoin, or refuses it: at most 64 KiB, at most 16 deep, UTF-8, a strict
  JSON reader of its own, no key twice, no bytes after the document, the
  price as exact decimal text rounded half up to the cent and inside a
  band.
- `combine(samples, now)` makes the samples of two or more sources into the
  counter's `Rate` and `RateSource`: only fresh samples, the median (the
  lower middle of an even count), every price within 2% of it or the call
  is refused, the label `med.` and one letter per source.
- `say` gives every refusal its words (`docs/screens/rates.md`).

Real replies of the four sources, fetched on 2026-11-11 from the public
endpoints, are in `tests/fixtures/`. The fuzz target is `rates_parse`.
