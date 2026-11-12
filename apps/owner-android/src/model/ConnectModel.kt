package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.owner.ImportedWalletView
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerImport
import org.openbmt.owner.OwnWalletView
import java.io.IOException

/** The screens of "Connect your wallet", in the order they come. */
enum class ConnectStep { Intro, Paste, Address, Different, Checked }

/**
 * "Connect your wallet" (the own wallet path): the wallet's output descriptor is scanned or pasted, the library reads
 * it, and the owner compares the first address with the one in their wallet. The state is in memory only: the text
 * is handed to the library and wiped, only the first address and the kind are kept until the owner says the address
 * is the same. Then the wallet's file, as the library pinned it, is handed to the app's store (OA-105); until then
 * nothing is saved.
 */
class Connect {
    var step = ConnectStep.Intro
        private set

    /** The first receive address of the wallet that read, until Different or a new reading. */
    var firstAddress: String? = null
        private set

    /** Whether the wallet that read is the 2-of-3 wallet. */
    var multisig = false
        private set

    private var pasted = false

    /** The wallet's file as the library made it (OA-105), held from the reading until it is saved or forgotten. */
    private var file: ByteArray? = null

    /** The paste screen, from the intro. */
    fun paste() {
        if (step == ConnectStep.Intro) step = ConnectStep.Paste
    }

    /**
     * Reads the [text] of a scan or a paste with [reader] (the library's one call) and wipes it. The address check
     * follows only when it reads; a refusal gives its words and the step stays. With a [pinner] (the library's call
     * that makes the wallet's file) the file is kept for [same]; one that does not pin is refused like one that does
     * not read.
     */
    fun read(
        text: ByteArray,
        reader: (String) -> ImportedWalletView = OwnerImport::read,
        pinner: ((String) -> OwnWalletView)? = null,
    ): SayWords? {
        if (step != ConnectStep.Intro && step != ConnectStep.Paste) {
            text.fill(0)
            return null
        }
        return try {
            val string = String(text, Charsets.UTF_8)
            val w = reader(string)
            val pinned = pinner?.invoke(string)
            file?.fill(0)
            file = pinned?.file
            pasted = step == ConnectStep.Paste
            firstAddress = w.firstAddress
            multisig = w.multisig
            step = ConnectStep.Address
            null
        } catch (e: OwnerException) {
            e.sayWords()
        } catch (e: LinkageError) {
            AppWords.libraryMissing(e.message)
        } finally {
            text.fill(0)
        }
    }

    /**
     * "Same address": the wallet is checked. With a [save] (the app's store) the wallet's file is written before the
     * step moves on, and the file is wiped from memory (OA-105); a write that fails leaves the step where it is and
     * gives the words.
     */
    fun same(save: ((ByteArray) -> Unit)? = null): SayWords? {
        if (step != ConnectStep.Address) return null
        val held = file
        if (save != null && held != null) {
            try {
                save(held)
            } catch (e: OwnerException) {
                return e.sayWords()
            } catch (e: IOException) {
                return OwnPairWords.walletNotSaved
            } catch (e: LinkageError) {
                return AppWords.libraryMissing(e.message)
            }
            held.fill(0)
            file = null
        }
        step = ConnectStep.Checked
        return null
    }

    /** "Different": it ends here and nothing of the reading is kept. */
    fun different() {
        if (step == ConnectStep.Address) {
            step = ConnectStep.Different
            forget()
        }
    }

    /** The screen before; false at the intro, where the caller leaves. */
    fun back(): Boolean {
        step = when (step) {
            ConnectStep.Intro -> return false
            ConnectStep.Paste -> ConnectStep.Intro
            ConnectStep.Address -> {
                forget()
                if (pasted) ConnectStep.Paste else ConnectStep.Intro
            }
            ConnectStep.Different -> ConnectStep.Intro
            ConnectStep.Checked -> ConnectStep.Address
        }
        return true
    }

    private fun forget() {
        file?.fill(0)
        file = null
        firstAddress = null
        multisig = false
    }

    companion object {
        /** The address in groups of four, three groups to a line, so that a long address sits in rows. */
        fun rows(address: String): String = address.chunked(4).chunked(3).joinToString("\n") { it.joinToString(" ") }
    }
}
