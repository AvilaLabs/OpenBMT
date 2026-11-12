package org.openbmt.owner.app

import android.view.View
import android.widget.Button
import org.openbmt.app.SayWords
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerImport

// The key maker guide of the first run (the savings wallet): one step to a screen beside the key maker page, the
// scan of the page's code and the two comparisons. Every sentence is in KeyGuideWords. The guide is in memory only:
// it writes no file, keeps nothing and never asks for or shows words. The scanned text goes to the library, which
// gives back a fingerprint and a first address and holds nothing; the text is not kept here either.

/** The screen the guide is at. [message] is what the last scan said, shown on the step that scans. */
internal fun Controller.showKeyGuide(r: FirstRun, message: SayWords? = null) {
    val g = r.guide
    fun go() = showFirstRun()
    val back: () -> Unit = {
        if (g.step == GuideStep.Stopped) g.restart() else if (!g.back()) r.back()
        go()
    }
    when (g.step) {
        GuideStep.Need -> guidePage(
            g, KeyGuideWords.kgNeedTitle,
            KeyGuideWords.kgNeedList.map { ui.text("•  $it", 15f, colour = ui.pal.secondary) } + para(KeyGuideWords.kgNeedCard),
            listOf(guideNext(g)), back,
        )
        GuideStep.Tails -> guidePage(
            g, KeyGuideWords.kgTailsTitle,
            listOf(para(KeyGuideWords.kgTailsText), line(KeyGuideWords.kgTailsNote, muted = true)),
            listOf(guideNext(g)), back,
        )
        GuideStep.Page -> guidePage(
            g, KeyGuideWords.kgPageTitle,
            listOf(para(KeyGuideWords.kgPageText), para(KeyGuideWords.kgPageCheck)),
            listOf(guideNext(g)), back,
        )
        GuideStep.Start -> guidePage(
            g, KeyGuideWords.kgStartTitle,
            listOf(para(KeyGuideWords.kgStartText), line(KeyGuideWords.kgStartNote, muted = true)),
            listOf(guideNext(g)), back,
        )
        GuideStep.Open -> guidePage(
            g, KeyGuideWords.kgOpenTitle, listOf(para(KeyGuideWords.kgOpenText)), listOf(guideNext(g)), back,
        )
        GuideStep.Follow -> guidePage(
            g, KeyGuideWords.kgFollowTitle,
            listOfNotNull(para(KeyGuideWords.kgFollowText), message?.let { ui.words(it) }),
            listOf(ui.primary(KeyGuideWords.kgScanButton) { scanGuide(r) }), back,
        )
        GuideStep.Compare -> guidePage(
            g, KeyGuideWords.kgCompareTitle,
            listOf(para(KeyGuideWords.kgCompareText)) + scanned(g),
            listOf(
                ui.primary(KeyGuideWords.kgCompareSame) { g.same(); go() },
                ui.button(KeyGuideWords.kgCompareDifferent) { g.different(); go() },
            ),
            back,
        )
        GuideStep.Electrum -> guidePage(
            g, KeyGuideWords.kgElectrumTitle,
            listOf(para(KeyGuideWords.kgElectrumText)) + scanned(g),
            listOf(
                ui.primary(KeyGuideWords.kgElectrumSame) { g.same(); go() },
                ui.button(KeyGuideWords.kgElectrumDifferent) { g.different(); go() },
            ),
            back,
        )
        GuideStep.Done -> guidePage(
            g, KeyGuideWords.kgDoneTitle, listOf(para(KeyGuideWords.kgDoneText)),
            listOf(ui.primary(KeyGuideWords.kgDoneButton) { r.next(); go() }), back,
        )
        GuideStep.Stopped -> guidePage(
            g, KeyGuideWords.kgStopTitle, listOf(para(KeyGuideWords.kgDifferentText)),
            listOf(ui.primary(KeyGuideWords.kgStartAgain) { back() }), back,
        )
    }
}

/** A page of the guide: the badge of a numbered step, the heading, the [body] and the [actions]. */
private fun Controller.guidePage(g: KeyGuide, title: String, body: List<View>, actions: List<Button>, back: () -> Unit) {
    val views = ArrayList<View>()
    g.step.number?.let { views.add(line(KeyGuideWords.badge(it, KeyGuide.TOTAL), muted = true)) }
    views.add(heading(title))
    views.addAll(body)
    stepScreen(KeyGuideWords.kgTitle, views, actions, back)
}

/** Next on a step, and Back on the screens that have a step before them is the bar's arrow and the system's Back. */
private fun Controller.guideNext(g: KeyGuide): Button = ui.primary(KeyGuideWords.kgNext) { g.next(); showFirstRun() }

/** What the phone read: the fingerprint large and the first address in groups of four, as the page shows them. */
private fun Controller.scanned(g: KeyGuide): List<View> = listOf(
    stack(label(KeyGuideWords.kgFingerprint), ui.text(g.fingerprint.orEmpty(), 28f, bold = true, mono = true)),
    stack(label(KeyGuideWords.kgFirstAddress), ui.text(KeyGuide.groups(g.firstAddress.orEmpty()), 18f, mono = true)),
)

/** The page's code, read by the camera, which is asked for only when the button is pressed. */
private fun Controller.scanGuide(r: FirstRun) {
    fun again(message: SayWords?) = showKeyGuide(r, message)
    fun read(bytes: ByteArray) {
        val shown = try {
            val w = OwnerImport.read(String(bytes, Charsets.UTF_8))
            if (r.guide.scanned(w.fingerprint, w.firstAddress)) null else KeyGuideWords.kgNotKeyMaker
        } catch (e: OwnerException) {
            e.sayWords()
        } catch (e: LinkageError) {
            AppWords.libraryMissing(e.message)
        } finally {
            bytes.fill(0)
        }
        if (shown == null) showFirstRun() else again(shown)
    }
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { read(it) },
        onFailed = { f -> again(f.words ?: KeyGuideWords.kgCameraOff) },
        onCancel = { again(null) },
    )
}
