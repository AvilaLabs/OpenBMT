package org.openbmt.counter.app

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.view.View
import android.widget.Button
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.OnePhone
import org.openbmt.app.OnePhoneCheck
import org.openbmt.app.SayWords
import org.openbmt.app.SealedFile
import org.openbmt.app.Ui
import org.openbmt.app.WordsModel
import org.openbmt.counter.CounterException
import org.openbmt.counter.OwnWalletKind
import org.openbmt.counter.Pairing

/**
 * Pairing with the owner app on the same phone (CR-277 to CR-282). Exported, guarded by a permission of protection
 * level signature, and no intent filter: the owner app starts it by its explicit name, for a result. It acts only for
 * a caller whose package is the owner app's and whose certificate is this app's own; anything else ends it with
 * words and changes nothing. The library calls are those of the camera's pairing (Controller): the offer is read
 * (CR-227), "Pair" stands for "Same address" and "They match" (CR-279), and the bundle is taken as a scanned one is
 * (CR-280). The ceremony stays in the library's memory between the two intents, as with the camera.
 */
class PairHereActivity : Activity() {
    private lateinit var ui: Ui
    private val main = Handler(Looper.getMainLooper())
    private val dir: File by lazy { noBackupFilesDir }
    private val pin by lazy { PinStore(dir, { SystemClock.elapsedRealtime() }, boot = { bootCount() }) }
    private val configFile by lazy { ConfigFile(dir) }

    /** The offer was handed back as a request: the ceremony is the owner app's to go on with, and is left in the library. */
    private var handedOver = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ui = Ui(this)
        // An activity made again by the system has lost what it was doing: it is not taken up again.
        if (savedInstanceState != null) return refuse(PairHereWords.NOT_OWNER)
        val caller = callingActivity?.packageName
        if (!OnePhone.callerAllowed(caller) || caller == null || !OnePhoneCheck.sameSigner(this, caller)) {
            return refuse(PairHereWords.NOT_OWNER)
        }
        when (intent?.action) {
            OnePhone.ACTION_OFFER -> offered(intent.getByteArrayExtra(OnePhone.EXTRA_OFFER) ?: ByteArray(0))
            OnePhone.ACTION_BUNDLE -> bundled(intent.getByteArrayExtra(OnePhone.EXTRA_BUNDLE) ?: ByteArray(0))
            else -> refuse(PairHereWords.NOT_OWNER)
        }
    }

    override fun onDestroy() {
        // Left before the request was handed back: the ceremony is forgotten, here and in the library.
        if (isFinishing && !handedOver && intent?.action == OnePhone.ACTION_OFFER) forget()
        super.onDestroy()
    }

    private fun nowSecs(): Long = System.currentTimeMillis() / 1000

    private fun bootCount(): Long = try {
        Settings.Global.getInt(contentResolver, Settings.Global.BOOT_COUNT).toLong()
    } catch (_: Settings.SettingNotFoundException) {
        -1L
    }

    private fun forget() {
        PairHereHold.clear()
        try {
            Pairing.cancel()
        } catch (_: CounterException) {
            // No ceremony in the library.
        } catch (_: LinkageError) {
            // No library, no ceremony.
        }
    }

    private fun put(body: List<View>, actions: List<Button>) {
        setContentView(ui.page(ui.bar(PairHereWords.TITLE), body, actions))
    }

    private fun say(text: String): View = ui.text(text, 16f)

    private fun sayWords(words: SayWords): View = ui.notice(WordsModel.of(words))

    /** Words, one button, and the activity ends with [result] when it is tapped (or Back is pressed). */
    private fun ending(body: View, result: Int, data: Intent? = null) {
        setResult(result, data)
        put(listOf(body), listOf(ui.primary(PairHereWords.DONE) { finish() }))
    }

    private fun refuse(text: String) = ending(say(text), RESULT_CANCELED)

    private fun refuse(words: SayWords) = ending(sayWords(words), RESULT_CANCELED)

    private fun refuse(e: Throwable) {
        refuse(
            when (e) {
                is CounterException -> e.sayWords()
                is LinkageError -> AppWords.libraryMissing(e.message)
                is IOException -> AppWords.fileNotSaved(e.message)
                is GeneralSecurityException -> AppWords.storageUnavailable(e.message)
                else -> throw e
            },
        )
    }

    private fun <T> async(work: () -> T, done: (Result<T>) -> Unit) {
        val t = Thread {
            val r = runCatching(work)
            main.post { if (!isDestroyed) done(r) }
        }
        t.isDaemon = true
        t.start()
    }

    // ---- PAIR_OFFER: read the offer, ask the person, make the request ---------------------------------------------

    private fun offered(offer: ByteArray) {
        if (File(dir, Controller.PAIRING_FILE).exists()) return refuse(PairHereWords.ALREADY)
        setContentView(ui.page(ui.bar(PairHereWords.TITLE), listOf(ui.working(WordsModel.of(AppWords.starting)))))
        // The counter's name is asked below when the counter is not set up yet; the library takes it again at "Pair".
        async({ Pairing.begin(offer, DEFAULT_NAME, nowSecs()) }) { r ->
            val started = r.getOrNull() ?: return@async refuse(r.exceptionOrNull() ?: IOException("the offer was not read"))
            if (started.wallet == OwnWalletKind.None) {
                // The Key app's offer is for the camera's way; nothing is kept.
                forget()
                return@async refuse(PairHereWords.NOT_TAKEN)
            }
            ask(SamePhone.offerIsStarter(offer))
        }
    }

    /** CR-278: before anything is made, the person is asked, on this screen. */
    private fun ask(starter: Boolean) {
        val setUp = pin.isSet() && try {
            configFile.load() != null
        } catch (_: IOException) {
            false
        }
        val body = ArrayList<View>()
        body.add(say(PairHereWords.TEXT))
        val message = Slot(this)
        // A counter that is not set up yet is asked what the first screen of the camera's way asks (CR-98).
        val counterName = ui.field("").also { it.setText(DEFAULT_NAME) }
        val manager = ui.field("")
        val pins = ui.pinPair("Manager's PIN, 6 to 12 digits", "PIN again")
        if (!setUp) {
            body.add(ui.labelled("Name of this counter", counterName))
            body.add(ui.labelled("Manager's name", manager))
            body.add(pins.view)
        }
        body.add(message)
        val pair = ui.primary(PairHereWords.PAIR) {
            val name = counterName.text.toString().trim()
            if (setUp) {
                pairNow(name, starter)
            } else {
                val problem = FirstStartCheck.problem(name, manager.text.toString().trim(), pins.first.text.toString(), pins.second.text.toString())
                if (problem != null) message.show(sayWords(problem)) else saveSetUp(name, manager.text.toString().trim(), pins.first.text.toString(), starter)
            }
        }
        val cancel = ui.button(PairHereWords.CANCEL) { cancelled() }
        put(body, listOf(pair, cancel))
    }

    private fun cancelled() {
        forget()
        setResult(RESULT_CANCELED)
        finish()
    }

    /** What the first screen of the camera's way keeps before it pairs: the PIN and the config (CR-98). */
    private fun saveSetUp(name: String, manager: String, pinText: String, starter: Boolean) {
        async({
            pin.set(pinText)
            configFile.save(AppConfig(AppConfig.DEFAULT_PROXY, manager, ""))
        }) { r ->
            val e = r.exceptionOrNull()
            if (e == null) pairNow(name, starter) else {
                forget()
                refuse(e)
            }
        }
    }

    /** "Pair": the counter makes its key and number, and the request goes back to the owner app only. */
    private fun pairNow(name: String, starter: Boolean) {
        async({ Pairing.addressSame(nowSecs(), name) }) { r ->
            val started = r.getOrNull()
            if (started == null) {
                forget()
                return@async refuse(r.exceptionOrNull() ?: IOException("the request was not made"))
            }
            PairHereHold.taken(starter)
            handedOver = true
            setResult(RESULT_OK, Intent().putExtra(OnePhone.EXTRA_REQUEST, started.request))
            finish()
        }
    }

    // ---- PAIR_BUNDLE: take the bundle, seal the record, keep the note ---------------------------------------------

    private fun bundled(bundle: ByteArray) {
        val starter = PairHereHold.take()
        if (starter == null) {
            // The app was stopped since the request: the ceremony is gone, and the owner app starts again.
            forget()
            return ending(sayWords(AppWords.pairingOwnForgotten), RESULT_OK, notPaired())
        }
        async({
            val kept = Pairing.finish(nowSecs(), bundle)
            SealedFile(dir, KeystoreSealer(Controller.KEY_ALIAS), Controller.PAIRING_FILE).save(kept)
            // The note is written after the record is sealed; a note that is not written leaves the ordinary line.
            try {
                SamePhone.write(dir, starter)
            } catch (_: IOException) {
                SamePhone.delete(dir)
            }
        }) { r ->
            val e = r.exceptionOrNull()
            if (e == null) {
                setResult(RESULT_OK, Intent().putExtra(OnePhone.EXTRA_PAIRED, true))
                val body = ArrayList<View>()
                body.add(ui.text(PairHereWords.PAIRED_TITLE, 24f, bold = true))
                body.add(say(PairHereWords.PAIRED_TEXT))
                put(body, listOf(ui.primary(PairHereWords.DONE) { finish() }))
            } else {
                // The ceremony is over once the library has taken a bundle, whatever came of it.
                forget()
                val words = when (e) {
                    is CounterException -> e.sayWords()
                    is LinkageError -> AppWords.libraryMissing(e.message)
                    is IOException -> AppWords.fileNotSaved(e.message)
                    is GeneralSecurityException -> AppWords.storageUnavailable(e.message)
                    else -> throw e
                }
                ending(sayWords(words), RESULT_OK, notPaired())
            }
        }
    }

    /** The result that says the counter is not paired, and nothing else (CR-280). */
    private fun notPaired(): Intent = Intent().putExtra(OnePhone.EXTRA_PAIRED, false)

    companion object {
        /** The name a counter that is already set up is given; the first screen of the camera's way offers the same. */
        private const val DEFAULT_NAME = "Counter"
    }
}
