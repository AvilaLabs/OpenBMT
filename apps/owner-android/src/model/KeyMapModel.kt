package org.openbmt.owner.app

import org.openbmt.app.Palette
import org.openbmt.app.Tone

/** Where one key is in the setup: made, the one to do next, or not yet reached. */
enum class KeyState { Made, Next, NotYet }

/**
 * What the key map at the top of the key-setup screens shows (KeyMap.kt draws it): the state of each of the three
 * keys at each stage of the ceremony, its words, and its colours, all from the palette. Key 1 is on this phone, key 2
 * on the Key app, key 3 on steel; the library's Key app answer carries key 3's public part (OC-41) and key 2's when
 * the Key app holds one. No Android class is used.
 */
object KeyMapModel {
    /** Below this width, in dp, the three cards stack one under the other. */
    const val STACK_BELOW_DP = 340

    /** The setup is done as far as keys go: the wallet is built (stage 2) or its words are confirmed (stage 3). */
    val allMade = listOf(KeyState.Made, KeyState.Made, KeyState.Made)

    /**
     * The three states, from what the ceremony holds. [stage] is the library's stage of the sealed record (1 key 1
     * made, 2 wallet built, 3 words confirmed); [answerRead] is whether the Key app's answer was read this time;
     * [rootChecked] is whether the owner said the Key app's words match; [answerHasKey2] is whether that answer
     * carried key 2.
     *
     * Before the answer: key 1 made, key 2 next, key 3 not yet. With the answer read but its words not yet
     * compared: keys 2 and 3 are next (their public parts are read, and taken only after the words match). Once the
     * words match, key 3 is taken, and key 2 is too unless it must come from another device. From the built wallet
     * on, all three are made.
     */
    fun states(stage: Int, answerRead: Boolean, rootChecked: Boolean, answerHasKey2: Boolean): List<KeyState> = when {
        stage >= 2 -> allMade
        !answerRead -> listOf(KeyState.Made, KeyState.Next, KeyState.NotYet)
        !rootChecked -> listOf(KeyState.Made, KeyState.Next, KeyState.Next)
        else -> listOf(KeyState.Made, if (answerHasKey2) KeyState.Made else KeyState.Next, KeyState.Made)
    }

    fun stateWord(s: KeyState): String = when (s) {
        KeyState.Made -> AppWords.keyMapMade
        KeyState.Next -> AppWords.keyMapNext
        KeyState.NotYet -> AppWords.keyMapNotYet
    }

    fun keyName(key: Int): String = when (key) {
        0 -> AppWords.keyMapKey1
        1 -> AppWords.keyMapKey2
        else -> AppWords.keyMapKey3
    }

    fun keyWhere(key: Int): String = when (key) {
        0 -> AppWords.keyMapKey1Where
        1 -> AppWords.keyMapKey2Where
        else -> AppWords.keyMapKey3Where
    }

    /** The accessibility description with the three states filled in, in lower case. */
    fun spoken(states: List<KeyState>): String {
        fun word(i: Int) = stateWord(states.getOrElse(i) { KeyState.NotYet }).lowercase()
        return AppWords.keyMapSpoken.replace("{k1}", word(0)).replace("{k2}", word(1)).replace("{k3}", word(2))
    }

    /** The tone of a key in a scheme: key 1 the blue (info), key 2 the green (good), key 3 the grey (wait). */
    fun tone(key: Int, night: Boolean): Tone {
        val s = Palette.of(night)
        return when (key) {
            0 -> s.info
            1 -> s.good
            else -> s.wait
        }
    }

    /**
     * The fill of a made key and of its chip, carrying white: the `waiting`, `paid` and `ended` bands in the light
     * scheme, and the blue, green and grey bands of the dark scheme in the dark one.
     */
    fun fill(key: Int, night: Boolean): Int {
        if (night) return tone(key, true).band
        return when (key) {
            0 -> Palette.waiting.band
            1 -> Palette.paid.band
            else -> Palette.ended.band
        }
    }

    /** The key's colour as a line or text on a card: an outline of the next key, and the text of its chip. */
    fun ink(key: Int, night: Boolean): Int = tone(key, night).ink

    /** The soft fill behind the next key's chip. */
    fun tint(key: Int, night: Boolean): Int = tone(key, night).tint

    /** The line of a key not reached yet: its ink at [NOT_YET_PART] of its strength over the card. */
    fun notYetLine(key: Int, night: Boolean): Int = mix(ink(key, night), Palette.of(night).surface, NOT_YET_PART)

    /** How much of the key's own colour a key not reached yet keeps: low contrast, still seen. */
    const val NOT_YET_PART = 0.55

    /** [a] and [b] blended opaque, [part] of [a]. */
    fun mix(a: Int, b: Int, part: Double): Int {
        fun ch(shift: Int): Int {
            val x = (a shr shift) and 0xFF
            val y = (b shr shift) and 0xFF
            return ((x * part + y * (1.0 - part)).toInt().coerceIn(0, 255)) shl shift
        }
        return 0xFF000000.toInt() or ch(16) or ch(8) or ch(0)
    }
}
