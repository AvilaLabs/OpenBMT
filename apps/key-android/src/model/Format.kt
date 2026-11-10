package org.openbmt.key.app

import java.text.SimpleDateFormat
import java.util.Date

/** Small text helpers shared by the screens. Plain Kotlin and the JDK only. */
object Format {
    /** The longest quoted text, in characters (SC-08). */
    const val MAX_QUOTED = 64

    /** An amount for a sentence: "12,345 sats" (SC-09). */
    fun sats(amount: Long): String {
        val digits = amount.toULong().toString()
        val out = StringBuilder()
        for ((i, c) in digits.withIndex()) {
            if (i > 0 && (digits.length - i) % 3 == 0) out.append(',')
            out.append(c)
        }
        out.append(if (amount == 1L) " sat" else " sats")
        return out.toString()
    }

    private fun hidden(c: Char): Boolean =
        c in '​'..'‏' || c in '‪'..'‮' || c in '⁠'..'⁩' || c == '﻿'

    /**
     * Text from another device made safe to show (SC-08): control characters
     * and line ends become spaces, characters that hide or turn text around
     * are dropped, runs of spaces become one, at most [MAX_QUOTED] characters.
     */
    fun cleanForeign(text: String): String {
        val out = StringBuilder()
        var space = true
        for (raw in text) {
            if (hidden(raw)) continue
            val c = if (Character.isISOControl(raw) || Character.isWhitespace(raw)) ' ' else raw
            if (c == ' ') {
                if (space) continue
                space = true
            } else {
                space = false
            }
            out.append(c)
            if (out.length >= MAX_QUOTED) break
        }
        return out.toString().trimEnd()
    }

    /** A time on the person's own clock, from seconds since 1970. */
    fun localTime(seconds: Long): String =
        SimpleDateFormat("yyyy-MM-dd HH:mm").format(Date(seconds * 1000L))

    /** The bytes of 2n hex digits, or null when the text is not exactly that. */
    fun fromHex(text: String): ByteArray? {
        val t = text.trim()
        if (t.length % 2 != 0) return null
        val out = ByteArray(t.length / 2)
        for (i in out.indices) {
            val hi = Character.digit(t[2 * i], 16)
            val lo = Character.digit(t[2 * i + 1], 16)
            if (hi < 0 || lo < 0) return null
            out[i] = ((hi shl 4) or lo).toByte()
        }
        return out
    }

    /** A whole number of at most 18 digits, or null. */
    fun toAmount(text: String): Long? {
        val t = text.trim().replace(",", "")
        if (t.isEmpty() || t.length > 18 || !t.all { it in '0'..'9' }) return null
        return t.toLong()
    }

    /** Heads and tails typed as H and T (any case, spaces allowed); null on any other character. */
    fun toFlips(text: String): List<Boolean>? {
        val out = ArrayList<Boolean>()
        for (c in text) {
            when (c) {
                'H', 'h' -> out.add(true)
                'T', 't' -> out.add(false)
                ' ', '\n', ',' -> {}
                else -> return null
            }
        }
        return out
    }

    /** Dice faces 1 to 6 typed in a row; anything else is dropped. */
    fun toRolls(text: String): String = text.filter { it in '1'..'6' }
}
