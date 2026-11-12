package org.openbmt.owner.app

import org.openbmt.app.Palette

/** Where one key is in the setup: made, the one to do next, or not yet reached. */
enum class KeyState { Made, Next, NotYet }

/**
 * What the key path at the top of the key-setup screens shows (KeyMap.kt draws it): the state of each of the three
 * keys at each stage of the ceremony, its words, and its colours (one blue for all three keys, a grey for a key not
 * reached), all from the palette. Key 1 is on this phone, key 2 on the Key app, key 3 on steel; the library's Key app answer carries key 3's public part (OC-41) and key 2's when
 * the Key app holds one. No Android class is used.
 */
object KeyMapModel {
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

    /** The size of each key's circle, and the width of its ring and of the line that joins it to the next key, in dp. */
    const val CIRCLE_DP = 64
    const val RING_DP = 3
    const val LINE_DP = 3

    /** The width of the soft halo round the next key, in dp. */
    const val HALO_DP = 6

    /** The one colour of the three keys as a fill (a made key's circle), carrying white: the `waiting` band, or the dark scheme's info band. */
    fun fill(night: Boolean): Int = if (night) Palette.dark.info.band else Palette.waiting.band

    /** The same colour as a line on the page (the next key's ring and glyph, the line after a made key): the band itself in the light scheme, the info ink in the dark one, whose band is too dim against the page. */
    fun line(night: Boolean): Int = if (night) Palette.dark.info.ink else Palette.waiting.band

    /** The soft halo of the next key: the band's second-line tint in the light scheme, the info tint in the dark one. */
    fun halo(night: Boolean): Int = if (night) Palette.dark.info.tint else Palette.waiting.sub

    /** The ring and the line of a key not reached yet: grey, the hint colour at [GREY_RING_PART] over the circle. */
    fun greyRing(night: Boolean): Int = mix(Palette.of(night).hint, Palette.of(night).surface, GREY_RING_PART)

    /** The key glyph of a key not reached yet: grey, the hint colour at [GREY_GLYPH_PART] over the circle. */
    fun greyGlyph(night: Boolean): Int = mix(Palette.of(night).hint, Palette.of(night).surface, GREY_GLYPH_PART)

    /** The name and the place of a key not reached yet: grey, halfway between the hint and the secondary colour, which reads on the page. */
    fun greyLabel(night: Boolean): Int = mix(Palette.of(night).hint, Palette.of(night).secondary, 0.5)

    /** How much of the hint colour the grey ring and the grey glyph keep. */
    const val GREY_RING_PART = 0.35
    const val GREY_GLYPH_PART = 0.6

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
