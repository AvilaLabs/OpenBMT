package org.openbmt.owner.app

import android.view.View
import org.openbmt.app.SayWords

// The setting "Tor" (OA-99): the page with the two choices, and the page that takes Orbot's address. The choice is
// saved before the app starts again with it; this file starts and stops nothing itself.

/** The page of the two choices. The words are those of the choice now in use; the other choice is a button. */
internal fun Controller.showTor(message: SayWords? = null) {
    val now = config?.tor ?: TorChoice.BuiltIn
    val views = ArrayList<View>()
    message?.let { views.add(ui.words(it)) }
    val actions = ArrayList<android.widget.Button>()
    for (choice in TorChoice.values()) {
        val go = { if (choice == TorChoice.Orbot) showTorOrbot(null) else chooseTor(choice, config?.proxy ?: AppConfig.DEFAULT_PROXY) { w -> showTor(w) } }
        actions.add(if (choice == now) ui.primary(choice.label) { go() } else ui.button(choice.label) { go() })
    }
    actions.add(ui.button("Back") { showHome() })
    screen(AppWords.torBuiltIn.heading, views, actions, back = { showHome() }, words = now.words)
}

/** The page of Orbot's address, prefilled with the one kept; saving it is choosing Use Orbot. */
private fun Controller.showTorOrbot(message: SayWords?) {
    val views = ArrayList<View>()
    views.add(ui.words(AppWords.torAlwaysOn))
    val proxy = ui.field("host:port", mono = true).also { it.setText(config?.proxy ?: AppConfig.DEFAULT_PROXY) }
    views.add(ui.labelled("Address of Orbot", proxy))
    screen(
        TorChoice.Orbot.label, views,
        listOf(
            ui.primary(TorChoice.Orbot.label) {
                val text = proxy.text.toString().trim()
                if (!HostPort.valid(text)) return@primary showTorOrbot(AppWords.torAddressNotAccepted)
                chooseTor(TorChoice.Orbot, text) { w -> showTorOrbot(w) }
            },
            ui.button("Back") { showTor() },
        ),
        back = { showTor() }, words = message ?: AppWords.torUsesOrbot,
    )
}
