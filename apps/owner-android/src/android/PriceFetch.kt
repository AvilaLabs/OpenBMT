package org.openbmt.owner.app

import android.widget.EditText
import org.openbmt.app.SayWords
import org.openbmt.owner.ShopCurrency

/**
 * The price a phone with no settings needs (OA-101): [slot] shows [fetching] while the phone asks the price
 * sources through Tor, then [fetched] with the price and the price in [field], or [needed] and an empty field
 * when the sources did not answer or did not agree. The owner can type another price at any time; what was
 * typed while the sources were asked is not replaced. The call waits on the network, so it runs off the screen
 * thread. Nothing here judges a price: the sources, the reading and the agreement are the library's.
 */
internal fun Controller.fillPrice(
    slot: Slot,
    field: EditText,
    fetching: SayWords,
    fetched: (Long) -> SayWords,
    needed: SayWords,
) {
    slot.show(ui.words(fetching))
    val gate = TorGate(config?.proxy ?: AppConfig.DEFAULT_PROXY)
    async({ PriceModel.fetch(LibraryPrice, HttpsRateFetcher(gate)) { nowSecs() } }) { r ->
        val outcome = r.getOrNull() ?: PriceOutcome.Missing
        if (outcome is PriceOutcome.Fetched) {
            slot.show(ui.words(fetched(outcome.usd)))
            if (field.text.isNullOrBlank()) field.setText(PriceModel.fieldText(outcome))
        } else {
            slot.show(ui.words(needed))
        }
    }
}

/**
 * As [fillPrice], for the price a cash-out is recorded at (OA-102): the sources of the shop's [currency] are
 * asked, and the price is in whole units of it. [fetched] gets that price.
 */
internal fun Controller.fillPriceIn(
    slot: Slot,
    field: EditText,
    currency: ShopCurrency,
    fetching: SayWords,
    fetched: (Long) -> SayWords,
    needed: SayWords,
) {
    slot.show(ui.words(fetching))
    val gate = TorGate(config?.proxy ?: AppConfig.DEFAULT_PROXY)
    async({ PriceModel.fetchIn(currency.code, LibraryPrice, HttpsRateFetcher(gate)) { nowSecs() } }) { r ->
        val outcome = r.getOrNull() ?: PriceOutcome.Missing
        if (outcome is PriceOutcome.FetchedIn) {
            slot.show(ui.words(fetched(outcome.whole)))
            if (field.text.isNullOrBlank()) field.setText(PriceModel.fieldText(outcome))
        } else {
            slot.show(ui.words(needed))
        }
    }
}
