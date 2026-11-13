package org.openbmt.counter.app

import android.app.Activity
import android.app.KeyguardManager
import android.content.ComponentName
import android.content.Context
import android.nfc.NfcAdapter
import android.nfc.cardemulation.CardEmulation
import android.nfc.cardemulation.HostApduService
import android.os.Bundle
import android.os.PowerManager
import org.openbmt.counter.CounterException
import org.openbmt.counter.Tap

/**
 * TP-10 to TP-15: the counter answers a reader as an NFC Forum Type 4 tag that holds the payment request. Every
 * command is answered by the library (`Tap.answer`); this class passes the bytes on and keeps the session between
 * the commands of one tap. It never logs a command, a response or the tag, and builds nothing of either.
 *
 * Android sends commands here only while the payment screen asks for it (TP-20, [TapPreferred]); the library answers
 * "not found" when [TapTag] is empty (TP-04). TP-21: nothing is answered while the screen is off or the phone is
 * locked, whatever the manifest asks for, checked on every command.
 */
class TapService : HostApduService() {
    /** What the library returned for the last command of this tap; empty at the start. */
    private var session = ByteArray(0)

    override fun processCommandApdu(commandApdu: ByteArray?, extras: Bundle?): ByteArray {
        val content = if (awake()) TapTag.current() else null
        return try {
            val answer = Tap.answer(session, content, commandApdu ?: ByteArray(0))
            session = answer.session
            answer.response
        } catch (e: CounterException) {
            refuse()
        } catch (e: LinkageError) {
            refuse()
        }
    }

    /** TP-15: the reader went away; the selection and the content it read are dropped. */
    override fun onDeactivated(reason: Int) {
        session = ByteArray(0)
    }

    /** "Not found", the answer for a tag that holds nothing, when the library cannot answer. */
    private fun refuse(): ByteArray {
        session = ByteArray(0)
        return byteArrayOf(0x6A, 0x82.toByte())
    }

    /** TP-21: the screen is on and the phone is unlocked. */
    private fun awake(): Boolean {
        val power = getSystemService(PowerManager::class.java)
        val keys = getSystemService(KeyguardManager::class.java)
        return power != null && keys != null && power.isInteractive && !keys.isDeviceLocked
    }
}

/**
 * TP-20: asks Android to send the commands of a reader to [TapService] while the payment screen of a sale is in
 * front. A phone without NFC, with NFC off or without card emulation takes the call as a no-op; it never fails.
 */
object TapPreferred {
    /** Prefer the tag service while [activity] is in front ([on]), or stop. */
    fun set(activity: Activity, on: Boolean) {
        try {
            val adapter = NfcAdapter.getDefaultAdapter(activity) ?: return
            val emulation = CardEmulation.getInstance(adapter)
            val service = ComponentName(activity, TapService::class.java)
            if (on) emulation.setPreferredService(activity, service) else emulation.unsetPreferredService(activity)
        } catch (e: RuntimeException) {
            // No NFC, no card emulation, or the activity is not in front: there is nothing to prefer.
        } catch (e: LinkageError) {
            // The platform has no such class.
        }
    }
}
