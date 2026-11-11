package org.openbmt.owner.app

import android.view.View
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.DurableFile
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.LogoSnapView
import org.openbmt.app.SayWords
import org.openbmt.app.SealedFile
import org.openbmt.app.UrCall
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
    centre(words, listOf(ui.primary("Try again") { keep(record, next) }, ui.button("Back") { begin() }), title = "Not saved", back = { begin() })
}

// ---- the first step: key 1 ----------------------------------------------------------------------------------------

/**
 * "Make a new shop wallet": a page that says it takes a moment is drawn first, then key 1 is made on the
 * screen thread (OA-12). Key 1 has no words to show (`show_words` is false).
 */
internal fun Controller.startCeremony() {
    ceremonyLogoShown = false
    waiting(AppWords.ceremonyMakingKey)
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
    centre(
        stateWords,
        listOf(ui.primary(ResumeAction.ScanAnswer.label) { scanAnswer() }, ui.button("Start again") { showStartAgain() }),
        extra = listOf(ui.words(AppWords.ceremonyScanNext)),
        title = "Key 1 is made", logo = false,
    )
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
    ceremonyLogoShown = false
    val views = ArrayList<View>()
    message?.let { views.add(ui.words(it)) }
    views.add(ui.words(at.words.say()))
    views.add(ui.words(CeremonyModel.resumeWords(action)))
    val go = ui.primary(action.label) {
        when (action) {
            ResumeAction.ScanAnswer, ResumeAction.ScanAnswerAgain -> scanAnswer()
            ResumeAction.SaveSetup -> saveFinished(record)
        }
    }
    screen("Setup in progress", views, listOf(go, ui.button("Start again") { showStartAgain() }), words = AppWords.ceremonyResume)
}

/** A record that does not read: the way out is to start again, which throws it away after a confirmation. */
private fun Controller.notReadable(words: SayWords) {
    centre(words, listOf(ui.primary("Try again") { begin() }, ui.button("Start again") { showStartAgain() }))
}

/** The confirmation before the unfinished ceremony is thrown away. */
internal fun Controller.showStartAgain() {
    screen("Start again?", emptyList(), listOf(ui.primary("Keep going") { begin() }, ui.danger("Start again") { throwAway() }), back = { begin() }, words = AppWords.ceremonyStartAgain)
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
        show = { v -> showScan(v) },
        onBytes = { give(SourceResult.Failed(AppWords.ceremonyNotAnAnswer)) },
        onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
        onCancel = { give(SourceResult.Failed(AppWords.ceremonyScanCancelled)) },
        ur = UrScan(CeremonyModel.ANSWER_TYPES, ExchangeModel.WHO) { _, payload -> give(CeremonyModel.answerResult(payload)) },
    )
}

/** What the scan or the library said about the answer, and the way to scan again. */
private fun Controller.showAnswerFailed(words: SayWords) {
    centre(
        words,
        listOf(ui.primary(ResumeAction.ScanAnswer.label) { scanAnswer() }, ui.button("Start again") { showStartAgain() }),
        title = "Scan the Key app",
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
    val body = listOf(stack(label("The Key app's identity words"), wordCards(CeremonyModel.wordsLine(read.keyAppRootWords))))
    val same = ui.primary("The Key app shows the same words") {
        if (read.key2 == null) showKey2(d, null) else buildWallet(d)
    }
    val differ = ui.danger("They differ") {
        d.answer = null
        d.read = null
        showAnswerFailed(AppWords.ceremonyRootDiffers)
    }
    screen("Check the Key app", body, listOf(same, ui.button("Start again") { showStartAgain() }, differ), words = AppWords.ceremonyCompareRoot)
}

// ---- the third step: key 2, when the Key app holds none --------------------------------------------------------------

/** Key 2 by scan (plain text or a key code) or paste; [message] is what the last try said. Never asked for when the answer holds it (OC-41). */
private fun Controller.showKey2(d: CeremonyDraft, message: SayWords?) {
    val pasted = ui.field("[fingerprint/path]key", lines = 4, mono = true)
    val use = ui.button("Use the pasted text") {
        when (val f = CeremonyModel.key2Text(pasted.text.toString())) {
            is Form.Refused -> showKey2(d, f.words)
            is Form.Ok -> {
                d.key2Text = f.value
                buildWallet(d)
            }
        }
    }
    screen(
        "Key 2", listOf(pasted, use),
        listOf(ui.primary("Scan key 2's code") { scanKey2(d) }, ui.button("Start again") { showStartAgain() }),
        words = message ?: AppWords.ceremonyKey2Needed,
    )
}

/** Key 2's code is plain text (its bytes are the text) or a UR of a key (read by the library, UR-31); the library reads the key. */
private fun Controller.scanKey2(d: CeremonyDraft) {
    scan.begin(
        show = { v -> showScan(v) },
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
        ur = UrScan(CeremonyModel.KEY2_TYPES, ExchangeModel.WHO) { type, payload ->
            when (val f = CeremonyModel.key2FromMessage(UrCall.native().keyText(type, payload))) {
                is Form.Refused -> showKey2(d, f.words)
                is Form.Ok -> {
                    d.key2Text = f.value
                    buildWallet(d)
                }
            }
        },
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
    waiting(AppWords.ceremonyBuilding)
    main.post {
        try {
            val defaults = OwnerCeremony.defaults(AppConfig.NETWORK)
            val built = OwnerCeremony.buildWallet(BuildArgs(d.record, answer, d.key2Text, defaults.batchMinCount))
            keep(built.record) {
                d.record = built.record
                d.built = built
                markKey2(built) { showWalletCheck(d, built) }
            }
        } catch (e: OwnerException) {
            showBuildRefused(d, e.sayWords())
        } catch (e: LinkageError) {
            showBuildRefused(d, AppWords.libraryMissing(e.message))
        }
    }
}

/**
 * Notes in a file of its own whether the pairs of keys say that key 2 is held on a hardware signer, before the
 * wallet is shown (OA-72); a wallet whose key 2 is in the Key app removes it. A file that cannot be written is
 * said, with a way to try again.
 */
private fun Controller.markKey2(built: WalletBuiltView, next: () -> Unit) {
    val file = File(appDir, AppFiles.KEY2_SIGNER_FILE)
    try {
        if (HardwareModel.key2IsSigner(built.pairs)) DurableFile.write(appDir, file, "1\n".toByteArray()) else file.delete()
        File(appDir, AppFiles.FIRST_ADDRESS_FILE).delete()
    } catch (e: IOException) {
        centre(AppWords.fileNotSaved(e.message), listOf(ui.primary("Try again") { markKey2(built, next) }, ui.button("Back") { begin() }), title = "Not saved")
        return
    }
    next()
}

/** The wallet was refused: the words, and the way back to the step that can change it. */
private fun Controller.showBuildRefused(d: CeremonyDraft, words: SayWords) {
    val actions = ArrayList<android.widget.Button>()
    actions.add(ui.primary(ResumeAction.ScanAnswer.label) { scanAnswer() })
    if (d.read?.key2 == null && d.answer != null) actions.add(ui.button("Give key 2 again") { showKey2(d, null) })
    actions.add(ui.button("Start again") { showStartAgain() })
    centre(words, actions)
}

/**
 * The wallet's facts for the owner: the checksum words large, the three fingerprints, the pairs of keys with
 * their holders in the library's words, and the first receive address.
 */
private fun Controller.showWalletCheck(d: CeremonyDraft, built: WalletBuiltView) {
    val views = ArrayList<View>()
    views.add(stack(label("Checksum words"), wordCards(CeremonyModel.wordsLine(built.checksumWords))))
    views.add(stack(label("Fingerprints"), *CeremonyModel.fingerprintLines(built.fingerprints).map { line(it, muted = true, mono = true) }.toTypedArray(), gap = 4))
    views.add(ui.words(built.pairsWords.say()))
    views.add(stack(*CeremonyModel.pairLines(built.pairs).map { line(it, muted = true) }.toTypedArray(), gap = 4))
    views.add(stack(label("First receive address"), line(built.firstReceiveAddress, muted = true, mono = true)))
    // The logo clicks together once, the first time this screen shows after the ceremony (OA-91).
    val snap = if (ceremonyLogoShown) null else LogoSnapView(act, 96).also { ceremonyLogoShown = true }
    snap?.let { views.add(0, centred(it)) }
    screen(
        "The new wallet", views,
        listOf(ui.primary("Show the Key app its setup") { showSetupCode(d, built) }, ui.button("Start again") { showStartAgain() }),
        words = AppWords.ceremonyCheckWallet,
    )
    snap?.post { snap.play() }
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
    val below = ArrayList<View>()
    val actions = ArrayList<android.widget.Button>()
    if (step.done) {
        below.add(ui.words(AppWords.ceremonyCompareSetup))
        below.add(wordCards(CeremonyModel.wordsLine(built.checksumWords)))
        below.addAll(CeremonyModel.fingerprintLines(built.fingerprints).map { line(it, muted = true, mono = true) })
        actions.add(ui.primary("The Key app shows the same words") { finishCeremony(d) })
        actions.add(ui.button("Back") { showWalletCheck(d, built) })
        actions.add(ui.danger("They differ") { showWordsDiffer(d, built) })
    } else {
        actions.add(ui.button("Show again") { showSetupCode(d, built) })
        actions.add(ui.button("Back") { showWalletCheck(d, built) })
    }
    codeScreen("Setup for the Key app", step.words, view, below, actions, back = { showWalletCheck(d, built) })
}

/** The owner says the words differ: the wallet is not finished, and only starting again on both phones goes on. */
private fun Controller.showWordsDiffer(d: CeremonyDraft, built: WalletBuiltView) {
    centre(
        AppWords.ceremonyWordsDiffer,
        listOf(ui.button("Start again") { showStartAgain() }, ui.button("Back") { showSetupCode(d, built) }),
        title = "Wallet not finished", back = { showSetupCode(d, built) },
    )
}

// ---- the last step: the owner's word, and the setup saved -----------------------------------------------------------

/**
 * The owner's word that the words are the same on both phones: the library finishes the setup with its own
 * refund depth and the first block of the network, the finished record is sealed, and the setup is saved.
 */
private fun Controller.finishCeremony(d: CeremonyDraft) {
    waiting(AppWords.ceremonyFinishing)
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
    centre(words, listOf(ui.primary("Try again") { finishCeremony(d) }, ui.button("Start again") { showStartAgain() }))
}

/**
 * Opens the finished record for the values the node starts from, writes them as the setup text the paste path
 * parses. [message] is what the last try said. The setup is then saved exactly as the paste path saves it, and
 * the ceremony's record is deleted only after that (OA-29); the start goes on as after a pasted setup.
 */
private fun Controller.saveFinished(record: ByteArray, message: SayWords? = null) {
    val text = try {
        OwnerSetup.format(OwnerCeremony.open(record))
    } catch (e: OwnerException) {
        return showSaveFailed(e.sayWords(), record)
    } catch (e: LinkageError) {
        return showSaveFailed(AppWords.libraryMissing(e.message), record)
    }
    val save = {
        saveSetup(
            text,
            onRefused = { w -> saveFinished(record, w) },
            working = AppWords.ceremonyFinishing,
            afterSaved = { File(appDir, AppFiles.CEREMONY_FILE).delete() },
            backWords = AppWords.ceremonySaveNext,
            keepsSignerMark = true,
        )
    }
    // OA-71: with key 2 on a signer, the wallet is shown to the signer as soon as the node is running. The
    // node does not run before the setup is saved, so the same save is made and the register screen follows.
    val actions = ArrayList<android.widget.Button>()
    if (File(appDir, AppFiles.KEY2_SIGNER_FILE).exists()) {
        actions.add(ui.primary("Register the wallet on your signer") { registerNext = true; save() })
        actions.add(ui.button("Save and start") { registerNext = false; save() })
    } else {
        actions.add(ui.primary("Save and start") { save() })
    }
    actions.add(ui.button("Start again") { showStartAgain() })
    screen("Save the setup", emptyList(), actions, words = message ?: AppWords.ceremonySaveNext)
}

private fun Controller.showSaveFailed(words: SayWords, record: ByteArray) {
    centre(words, listOf(ui.primary("Try again") { saveFinished(record) }, ui.button("Start again") { showStartAgain() }))
}
