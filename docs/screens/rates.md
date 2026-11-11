# Rates: what a person is told

Made by `screens_markdown` in `crates/openbmt-rates/src/say.rs`. Do not
edit it by hand: run the test `sc_12_the_list_is_on_disk` with the
environment variable `OPENBMT_WRITE_SCREENS=1` to write it again.

| State | Heading | What happened | What to do next | Who | Code |
|---|---|---|---|---|---|
| PriceRefused::Empty | Price missing | A price source sent an empty price, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-20 |
| PriceRefused::Sign | Price has a sign | A price source sent a price with a plus or minus sign, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-20 |
| PriceRefused::Exponent | Price in odd writing | A price source sent a price written with an exponent, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-20 |
| PriceRefused::BadCharacter | Price has odd characters | A price source sent a price with more than digits and a dot, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-20 |
| PriceRefused::TwoDots | Price has two dots | A price source sent a price with more than one dot, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-20 |
| PriceRefused::BadShape | Price badly written | A price source sent a price with a dot that has no digit beside it, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-20 |
| PriceRefused::LeadingZero | Price has extra zeros | A price source sent a price that starts with a zero it should not have, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-20 |
| PriceRefused::TooManyDecimals | Price too exact | A price source sent a price with more than eight digits after the dot, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-21 |
| PriceRefused::Zero | Price is zero | A price source sent a price of nothing, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-23 |
| PriceRefused::OutOfBand | Price out of range | A price source sent a price far above or far below what this app believes, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-24 |
| ParseRefused::TooLong | Price reply too long | A price source sent a reply longer than this app reads, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-10 |
| ParseRefused::TooDeep | Price reply too tangled | A price source sent a reply nested deeper than this app reads, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-11 |
| ParseRefused::NotUtf8 | Price reply unreadable | A price source sent a reply that is not plain text, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-12 |
| ParseRefused::Malformed | Price reply not understood | A price source sent a reply that is not written as expected, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-13 |
| ParseRefused::TrailingBytes | Price reply has extra bytes | A price source sent more after the end of its reply, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-13 |
| ParseRefused::DuplicateKey | Price reply repeats a field | A price source named one field twice in its reply, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-14 |
| ParseRefused::MissingField | Price reply lacks the price | A price source sent a reply without the field that holds the price, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-15 |
| ParseRefused::WrongShape | Price reply in a new form | A price source sent a reply in a form this app does not know, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-15 |
| ParseRefused::WrongCurrency | Price in another currency | A price source sent a price in a currency other than the shop's, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-17 |
| ParseRefused::SourceError | Price source reports a fault | A price source said that it could not give a price, so it was not used. | Nothing to do. The app asks again soon. If it keeps happening, report it to the OpenBMT project. | nobody | RT-16 |
| RatesRefused::TooOld | Price out of date | The price is not available right now: the last prices came in too long ago. | Take another kind of payment. Tell the owner if this lasts. | the cashier | RT-37 |
| RatesRefused::NotEnoughSources | Price not available | The price is not available right now: fewer than two price sources have answered. | Take another kind of payment. Try Bitcoin again in a minute. | the cashier | RT-31 |
| RatesRefused::PricesDisagree | Prices disagree | The price is not available right now: the price sources do not agree with each other. | Take another kind of payment. Tell the owner if this lasts. | the cashier | RT-33 |
| RatesRefused::MixedCurrencies | Prices in two currencies | The price is not available right now: the price sources gave prices in different currencies. | Take another kind of payment. Tell the owner: the price sources are set up wrongly. | the cashier | RT-38 |
