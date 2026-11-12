package org.openbmt.owner.app

import android.view.View
import android.widget.Button
import org.openbmt.app.CameraWords
import org.openbmt.app.SayWords
import org.openbmt.app.Who

// "Connect your wallet" of the first run, on the own wallet path: the wallet's output descriptor is scanned or
// pasted, read by the library (which gives back a first address and holds nothing) and the owner compares the
// address with their wallet. In memory only: nothing is saved here. Every sentence is in ConnectWords; a description
// that does not read gives the library's own words, as in the key maker guide.

/** The screen "Connect your wallet" is at. [message] is what the last reading said, shown on the screen that read. */
internal fun Controller.showConnect(r: FirstRun, message: SayWords? = null) {
    val c = r.connect
    fun go() = showFirstRun()
    val back: () -> Unit = { if (!c.back()) r.back(); go() }
    when (c.step) {
        ConnectStep.Intro -> stepScreen(
            FirstRunWords.shopTitle,
            listOfNotNull(
                heading(ConnectWords.introTitle), para(ConnectWords.introText),
                stack(
                    label(ConnectWords.introWhere),
                    *ConnectWords.introSteps.mapIndexed { i, t -> ui.text("${i + 1}.  $t", 15f) }.toTypedArray(),
                    gap = 6,
                ),
                line(ConnectWords.introNote, muted = true), message?.let { ui.words(it) },
            ),
            listOf(ui.primary(ConnectWords.introScan) { scanConnect(r) }, ui.button(ConnectWords.introPaste) { c.paste(); go() }), back,
        )
        ConnectStep.Paste -> showConnectPaste(r, message, back)
        ConnectStep.Address -> stepScreen(
            FirstRunWords.shopTitle,
            listOf(
                heading(ConnectWords.addressTitle), para(ConnectWords.addressText),
                ui.text(Connect.rows(c.firstAddress.orEmpty()), 28f, bold = true, mono = true),
                line(ConnectWords.kind(c.multisig), muted = true),
            ),
            listOf(
                ui.primary(ConnectWords.addressSame) { c.same(); go() },
                ui.button(ConnectWords.addressDifferent) { c.different(); go() },
            ),
            back,
        )
        ConnectStep.Different -> stepScreen(
            FirstRunWords.shopTitle,
            listOf(heading(ConnectWords.differentTitle), para(ConnectWords.differentText), ui.text(ConnectWords.differentNext, 15f, bold = true)),
            listOf(ui.button(KeyGuideWords.kgBack) { back() }), back, tone = ui.pal.stop,
        )
        ConnectStep.Checked -> stepScreen(
            FirstRunWords.shopTitle,
            listOf(heading(ConnectWords.checkedTitle), para(ConnectWords.checkedText), line(ConnectWords.checkedNote, muted = true)),
            listOf(ui.primary(ConnectWords.checkedNext) { r.next(); go() }), back, tone = ui.pal.good,
        )
    }
}

/** The text of the descriptor typed or pasted: it is cleared from the field and wiped after it is read. */
private fun Controller.showConnectPaste(r: FirstRun, message: SayWords?, back: () -> Unit) {
    val field = ui.field(ConnectWords.pasteHint, 6, mono = true)
    // The field's look sets a minimum height in pixels, which replaces the line count: it is set again.
    field.minLines = 6
    field.gravity = android.view.Gravity.TOP or android.view.Gravity.START
    val check: Button = ui.primary(ConnectWords.pasteCheck) {
        val bytes = field.text.toString().toByteArray(Charsets.UTF_8)
        field.setText("")
        val said = r.connect.read(bytes)
        if (said == null) showFirstRun() else showConnect(r, said)
    }
    val views = ArrayList<View>()
    views.add(heading(ConnectWords.introTitle))
    views.add(field)
    message?.let { views.add(ui.words(it)) }
    stepScreen(FirstRunWords.shopTitle, views, listOf(check), back)
}

/** The wallet's code, read by the camera, which is asked for only when the button is pressed. */
private fun Controller.scanConnect(r: FirstRun) {
    fun again(message: SayWords?) = showConnect(r, message)
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { bytes ->
            val said = r.connect.read(bytes)
            if (said == null) showFirstRun() else again(said)
        },
        onFailed = { f -> again(f.words ?: if (f.off) CameraWords.off(Who.OWNER) else CameraWords.notStarted(Who.OWNER, f.problem)) },
        onCancel = { again(null) },
    )
}
