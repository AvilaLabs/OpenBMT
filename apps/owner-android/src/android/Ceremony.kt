package org.openbmt.owner.app

import android.view.View
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.SayWords
import org.openbmt.app.SealedFile
import org.openbmt.app.UrScan
import org.openbmt.owner.AnswerView
import org.openbmt.owner.BuildArgs
import org.openbmt.owner.FinishArgs
import org.openbmt.owner.Key1Args
import org.openbmt.owner.OwnerCeremony
import org.openbmt.owner.OwnerException
import org.openbmt.owner.WalletBuiltView

// Making a new shop wallet on this phone (OA-26 to OA-31), with the Key app on a second phone: key 1 is made,
// the Key app's answer is scanned and its words compared, key 2 is taken when the Key app holds none, the
// wallet is built, its setup is shown to the Key app as a code, the owner compares the words on both phones,
// and the setup is saved as the pasted text is. Every judgment and every number is the library's
// (`OwnerCeremony`); this file shows its answers and carries the owner's choices. The record the library
// keeps between two steps is sealed on this phone before the next screen is shown. It signs nothing.

/** What the screens of one ceremony hold in memory. The record is the library's, sealed on disk at each step. */
internal class CeremonyDraft(var record: ByteArray) {
    var answer: ByteArray? = null
    var read: AnswerView? = null
    var key2Text: String? = null
    var built: WalletBuiltView? = null
}

private fun Controller.ceremonyFile() = SealedFile(appDir, KeystoreSealer(AppFiles.KEY_ALIAS), AppFiles.CEREMONY_FILE)

/**
 * Seals [record] as the ceremony's record, then goes on to [next]. The next screen is not shown before the
 * record is on disk: a record that cannot be saved is said so, with a button to try again (OA-26).
 */
private fun Controller.keep(record: ByteArray, next: () -> Unit) {
    async({ ceremonyFile().save(record) }) { r ->
        when (val e = r.exceptionOrNull()) {
            null -> next()
            is IOException -> notKept(AppWords.fileNotSaved(e.message), record, next)
            is GeneralSecurityException -> notKept(AppWords.storageUnavailable(e.message), record, next)
            else -> throw e
        }
    }
}

private fun Controller.notKept(words: SayWords, record: ByteArray, next: () -> Unit) {
    page(listOf(ui.words(words), ui.button("Try again") { keep(record, next) }, ui.button("Back") { begin() }))
}

// ---- the first step: key 1 ----------------------------------------------------------------------------------------

/**
 * "Make a new shop wallet": a page that says it takes a moment is drawn first, then key 1 is made on the
 * screen thread (OA-12). Key 1 has no words to show (`show_words` is false).
 */
internal fun Controller.startCeremony() {
    page(listOf(ui.words(AppWords.ceremonyMakingKey)))
    main.post {
        try {
            val made = OwnerCeremony.makeKey1(Key1Args(AppConfig.NETWORK, showWords = false))
            keep(made.record) {
                ceremony = CeremonyDraft(made.record)
                showKeyMade(made.stateWords.say())
            }
        } catch (e: OwnerException) {
            showFirstStart(e.sayWords())
        } catch (e: LinkageError) {
            showFirstStart(AppWords.libraryMissing(e.message))
        }
    }
}

/** Key 1 is made: the library's words of the stage, and what to do on the Key app. */
private fun Controller.showKeyMade(stateWords: SayWords) {
    val views = ArrayList<View>()
    views.add(ui.text("Key 1 is made", 22f, bold = true))
    views.add(ui.words(stateWords))
    views.add(ui.words(AppWords.ceremonyScanNext))
    views.add(ui.button(ResumeAction.ScanAnswer.label) { scanAnswer() })
    views.add(ui.button("Start again") { showStartAgain() })
    page(views)
}

// ---- an unfinished ceremony ------------------------------------------------------------------------------------------

/**
 * The start found a sealed record and no setup: the library says which stage it is at (`ceremonyStage`) and the
 * ceremony goes on from there, or is started again after a confirmation (OA-27).
 */
internal fun Controller.showCeremonyResume(message: SayWords?) {
    val record = try {
        ceremonyFile().load()
    } catch (e: IOException) {
        return notReadable(AppWords.ceremonyRecordNotRead(e.message))
    } catch (e: GeneralSecurityException) {
        return notReadable(AppWords.storageUnavailable(e.message))
    } ?: return notReadable(AppWords.ceremonyRecordNotRead("the record is missing"))
    val at = try {
        OwnerCeremony.stage(record)
    } catch (e: OwnerException) {
        return notReadable(AppWords.ceremonyRecordNotRead(e.rule))
    } catch (e: LinkageError) {
        return notReadable(AppWords.libraryMissing(e.message))
    }
    val action = CeremonyModel.resumeAction(at.stage) ?: return notReadable(AppWords.ceremonyRecordNotRead("stage ${at.stage}"))
    ceremony = CeremonyDraft(record)
    val views = ArrayList<View>()
    views.add(ui.text("Setup in progress", 22f, bold = true))
    message?.let { views.add(ui.words(it)) }
    views.add(ui.words(AppWords.ceremonyResume))
    views.add(ui.words(at.words.say()))
    views.add(ui.words(CeremonyModel.resumeWords(action)))
    views.add(
        ui.button(action.label) {
            when (action) {
                ResumeAction.ScanAnswer, ResumeAction.ScanAnswerAgain -> scanAnswer()
                ResumeAction.SaveSetup -> saveFinished(record)
            }
        },
    )
    views.add(ui.button("Start again") { showStartAgain() })
    page(views)
}

/** A record that does not read: the way out is to start again, which throws it away after a confirmation. */
private fun Controller.notReadable(words: SayWords) {
    page(listOf(ui.words(words), ui.button("Start again") { showStartAgain() }, ui.button("Try again") { begin() }))
}

/** The confirmation before the unfinished ceremony is thrown away. */
internal fun Controller.showStartAgain() {
    val views = ArrayList<View>()
    views.add(ui.text("Start again?", 22f, bold = true))
    views.add(ui.words(AppWords.ceremonyStartAgain))
    views.add(ui.button("Start again") { throwAway() })
    views.add(ui.button("Keep going") { begin() })
    page(views)
}

/** Deletes the ceremony's record. Nothing else on the phone is touched. */
private fun Controller.throwAway() {
    val file = File(appDir, AppFiles.CEREMONY_FILE)
    if (file.exists() && !file.delete()) {
        showMessage(AppWords.fileNotSaved("the record could not be removed")) { showStartAgain() }
        return
    }
    ceremony = null
    showMessage(AppWords.ceremonyThrownAway, "Continue") { begin() }
}

// ---- the second step: the Key app's answer -------------------------------------------------------------------------

/** The Key app's answer, read by the camera, which is asked for only when the button is pressed. */
private fun Controller.scanAnswer() {
    fun give(r: SourceResult) = when (r) {
        is SourceResult.Bytes -> readAnswer(r.bytes)
        is SourceResult.Failed -> showAnswerFailed(r.words)
    }
    scan.begin(
        show = { v ->
            tick = null
            act.show(v)
        },
        onBytes = { give(SourceResult.Failed(AppWords.ceremonyNotAnAnswer)) },
        onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
        onCancel = { give(SourceResult.Failed(AppWords.ceremonyScanCancelled)) },
        ur = UrScan(CeremonyModel.ANSWER_TYPES, ExchangeModel.WHO) { _, payload -> give(CeremonyModel.answerResult(payload)) },
    )
}

/** What the scan or the library said about the answer, and the way to scan again. */
private fun Controller.showAnswerFailed(words: SayWords) {
    page(
        listOf(
            ui.words(words),
            ui.button(ResumeAction.ScanAnswer.label) { scanAnswer() },
            ui.button("Start again") { showStartAgain() },
        ),
    )
}

/** The library reads and checks the answer; the Key app's four words are shown large for the owner to compare. */
private fun Controller.readAnswer(bytes: ByteArray) {
    val d = ceremony
    if (d == null) {
        begin()
        return
    }
    try {
        val read = OwnerCeremony.readAnswer(bytes)
        d.answer = bytes
        d.read = read
        d.key2Text = null
        showRootCheck(d, read)
    } catch (e: OwnerException) {
        showAnswerFailed(e.sayWords())
    } catch (e: LinkageError) {
        showAnswerFailed(AppWords.libraryMissing(e.message))
    }
}

private fun Controller.showRootCheck(d: CeremonyDraft, read: AnswerView) {
    val views = ArrayList<View>()
    views.add(ui.text("Check the Key app", 22f, bold = true))
    views.add(label("The Key app's identity words"))
    views.add(ui.text(CeremonyModel.wordsLine(read.keyAppRootWords), 30f, bold = true))
    views.add(ui.words(AppWords.ceremonyCompareRoot))
    views.add(
        ui.button("The Key app shows the same words") {
            if (read.key2 == null) showKey2(d, null) else buildWallet(d)
        },
    )
    views.add(
        ui.button("They differ") {
            d.answer = null
            d.read = null
            showAnswerFailed(AppWords.ceremonyRootDiffers)
        },
    )
    views.add(ui.button("Start again") { showStartAgain() })
    page(views)
}

// ---- the third step: key 2, when the Key app holds none --------------------------------------------------------------

/** Key 2 by scan (plain text) or paste; [message] is what the last try said. Never asked for when the answer holds it (OC-41). */
private fun Controller.showKey2(d: CeremonyDraft, message: SayWords?) {
    val views = ArrayList<View>()
    views.add(ui.text("Key 2", 22f, bold = true))
    views.add(ui.words(message ?: AppWords.ceremonyKey2Needed))
    val pasted = ui.field("[fingerprint/path]key", lines = 4)
    views.add(pasted)
    views.add(
        ui.button("Use the pasted text") {
            when (val f = CeremonyModel.key2Text(pasted.text.toString())) {
                is Form.Refused -> showKey2(d, f.words)
                is Form.Ok -> {
                    d.key2Text = f.value
                    buildWallet(d)
                }
            }
        },
    )
    views.add(ui.button("Scan key 2's code") { scanKey2(d) })
    views.add(ui.button("Start again") { showStartAgain() })
    page(views)
}

/** Key 2's code is plain text: the bytes of the scan are the text, and the library reads the key. */
private fun Controller.scanKey2(d: CeremonyDraft) {
    scan.begin(
        show = { v ->
            tick = null
            act.show(v)
        },
        onBytes = { bytes ->
            when (val f = CeremonyModel.key2FromScan(bytes)) {
                is Form.Refused -> showKey2(d, f.words)
                is Form.Ok -> {
                    d.key2Text = f.value
                    buildWallet(d)
                }
            }
        },
        onFailed = { f ->
            val r = ScanResult.failed(f.off, f.problem, f.words)
            showKey2(d, (r as? SourceResult.Failed)?.words ?: AppWords.ceremonyKey2Text)
        },
        onCancel = { showKey2(d, AppWords.ceremonyKey2ScanCancelled) },
    )
}

// ---- the fourth step: the wallet ---------------------------------------------------------------------------------------

/**
 * Builds the wallet from key 1, key 2 and key 3. It takes a moment, so a page says so and is drawn first; the
 * call stays on the screen thread (OA-12). The batch minimum is the library's own default, asked for.
 */
private fun Controller.buildWallet(d: CeremonyDraft) {
    val answer = d.answer
    if (answer == null) {
        showAnswerFailed(AppWords.ceremonyScanAgain)
        return
    }
    page(listOf(ui.words(AppWords.ceremonyBuilding)))
    main.post {
        try {
            val defaults = OwnerCeremony.defaults(AppConfig.NETWORK)
            val built = OwnerCeremony.buildWallet(BuildArgs(d.record, answer, d.key2Text, defaults.batchMinCount))
            keep(built.record) {
                d.record = built.record
                d.built = built
                showWalletCheck(d, built)
            }
        } catch (e: OwnerException) {
            showBuildRefused(d, e.sayWords())
        } catch (e: LinkageError) {
            showBuildRefused(d, AppWords.libraryMissing(e.message))
        }
    }
}

/** The wallet was refused: the words, and the way back to the step that can change it. */
private fun Controller.showBuildRefused(d: CeremonyDraft, words: SayWords) {
    val views = ArrayList<View>()
    views.add(ui.words(words))
    if (d.read?.key2 == null && d.answer != null) views.add(ui.button("Give key 2 again") { showKey2(d, null) })
    views.add(ui.button(ResumeAction.ScanAnswer.label) { scanAnswer() })
    views.add(ui.button("Start again") { showStartAgain() })
    page(views)
}

/**
 * The wallet's facts for the owner: the checksum words large, the three fingerprints, the pairs of keys with
 * their holders in the library's words, and the first receive address.
 */
private fun Controller.showWalletCheck(d: CeremonyDraft, built: WalletBuiltView) {
    val views = ArrayList<View>()
    views.add(ui.text("The new wallet", 22f, bold = true))
    views.add(ui.words(AppWords.ceremonyCheckWallet))
    views.add(label("Checksum words"))
    views.add(ui.text(CeremonyModel.wordsLine(built.checksumWords), 30f, bold = true))
    views.add(label("Fingerprints"))
    for (line in CeremonyModel.fingerprintLines(built.fingerprints)) views.add(ui.text(line, 14f, mono = true))
    views.add(ui.words(built.pairsWords.say()))
    for (line in CeremonyModel.pairLines(built.pairs)) views.add(ui.text(line, 14f))
    views.add(label("First receive address"))
    views.add(ui.text(built.firstReceiveAddress, 14f, mono = true))
    views.add(ui.button("Show the Key app its setup") { showSetupCode(d, built) })
    views.add(ui.button("Start again") { showStartAgain() })
    page(views)
}

// ---- the fifth step: the setup for the Key app, and the comparison -------------------------------------------------

/**
 * The wallet setup the Key app takes, as `ur:bytes` parts of the shared `AnimatedQrView`, with the checksum
 * words and the fingerprints for the owner to compare with the Key app's (OC-43). The owner's word that they
 * are the same is the only way on.
 */
private fun Controller.showSetupCode(d: CeremonyDraft, built: WalletBuiltView) {
    val shown = AnimatedQrView.make(
        act, ExchangeModel.WHO, ExchangeModel.TYPE_BYTES, built.walletSetup,
        ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels),
    )
    val view = shown.view
    val step = CeremonyModel.setupCodeStep(view != null, view?.animated == true)
    val views = ArrayList<View>()
    views.add(ui.text("Setup for the Key app", 22f, bold = true))
    views.add(ui.words(step.words))
    if (view != null) views.add(view)
    if (step.done) {
        views.add(ui.words(AppWords.ceremonyCompareSetup))
        views.add(ui.text(CeremonyModel.wordsLine(built.checksumWords), 30f, bold = true))
        for (line in CeremonyModel.fingerprintLines(built.fingerprints)) views.add(ui.text(line, 14f, mono = true))
        views.add(ui.button("The Key app shows the same words") { finishCeremony(d) })
        views.add(ui.button("They differ") { showWordsDiffer(d, built) })
    } else {
        views.add(ui.button("Show again") { showSetupCode(d, built) })
    }
    views.add(ui.button("Back") { showWalletCheck(d, built) })
    page(views)
}

/** The owner says the words differ: the wallet is not finished, and only starting again on both phones goes on. */
private fun Controller.showWordsDiffer(d: CeremonyDraft, built: WalletBuiltView) {
    val views = ArrayList<View>()
    views.add(ui.words(AppWords.ceremonyWordsDiffer))
    views.add(ui.button("Start again") { showStartAgain() })
    views.add(ui.button("Back") { showSetupCode(d, built) })
    page(views)
}

// ---- the last step: the owner's word, and the setup saved -----------------------------------------------------------

/**
 * The owner's word that the words are the same on both phones: the library finishes the setup with its own
 * refund depth and the first block of the network, the finished record is sealed, and the setup is saved.
 */
private fun Controller.finishCeremony(d: CeremonyDraft) {
    page(listOf(ui.words(AppWords.ceremonyFinishing)))
    main.post {
        try {
            val defaults = OwnerCeremony.defaults(AppConfig.NETWORK)
            val done = OwnerCeremony.finish(FinishArgs(d.record, defaults.refundDepth, defaults.birthday))
            keep(done.record) {
                d.record = done.record
                saveFinished(done.record)
            }
        } catch (e: OwnerException) {
            showFinishFailed(e.sayWords(), d)
        } catch (e: LinkageError) {
            showFinishFailed(AppWords.libraryMissing(e.message), d)
        }
    }
}

private fun Controller.showFinishFailed(words: SayWords, d: CeremonyDraft) {
    val views = ArrayList<View>()
    views.add(ui.words(words))
    views.add(ui.button("Try again") { finishCeremony(d) })
    views.add(ui.button("Start again") { showStartAgain() })
    page(views)
}

/**
 * Opens the finished record for the values the node starts from, writes them as the setup text the paste path
 * parses, and saves it exactly as the paste path does. The ceremony's record is deleted only after the setup
 * is saved (OA-29); the start goes on as after a pasted setup.
 */
private fun Controller.saveFinished(record: ByteArray) {
    val text = try {
        OwnerSetup.format(OwnerCeremony.open(record))
    } catch (e: OwnerException) {
        return showSaveFailed(e.sayWords(), record)
    } catch (e: LinkageError) {
        return showSaveFailed(AppWords.libraryMissing(e.message), record)
    }
    saveSetup(
        text, AppConfig.DEFAULT_PROXY,
        onRefused = { w -> showSaveFailed(w, record) },
        working = AppWords.ceremonyFinishing,
        afterSaved = { File(appDir, AppFiles.CEREMONY_FILE).delete() },
    )
}

private fun Controller.showSaveFailed(words: SayWords, record: ByteArray) {
    page(listOf(ui.words(words), ui.button("Try again") { saveFinished(record) }, ui.button("Start again") { showStartAgain() }))
}
