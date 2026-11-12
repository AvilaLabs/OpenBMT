package org.openbmt.app

/**
 * The colours of the three apps, each with a light and a dark value, chosen
 * from the phone's night mode. No Android class is used, so the pairs that are
 * drawn on each other can be checked for contrast on the desktop JVM
 * (tests/PaletteTest.kt), and the resource files of the themes
 * (res/values/colors.xml and res/values-night/colors.xml) are made from here.
 *
 * A [Tone] has three values: the [band], a fill that carries white text, the
 * [ink], the same tone as text or a line on the page's own background, and the
 * [tint], the soft fill behind a tinted panel (the ink and the text read on it).
 */
class Tone(val band: Int, val ink: Int, val tint: Int)

/** The full-screen colour of a counter's sale outcome, and the colour of its second line. White text is drawn on the [band]. */
class OutcomeBand(val band: Int, val sub: Int)

/** One set of colours: either the light one or the dark one. */
class Scheme(
    val background: Int,
    /** The page of the real cases: a warm off-white (dark: a warm near-black) under serif text. */
    val paper: Int,
    val surface: Int,
    val text: Int,
    val secondary: Int,
    val hint: Int,
    val divider: Int,
    /** The outline of a field. */
    val fieldLine: Int,
    /** The fill of a pill that is not selected, and of a small label. */
    val chip: Int,
    val good: Tone,
    val info: Tone,
    val wait: Tone,
    val warn: Tone,
    val stop: Tone,
) {
    /** The tone of a words colour (SC-06): blue is info, green good, red stop, neutral wait. */
    fun of(colour: Colour): Tone = when (colour) {
        Colour.Blue -> info
        Colour.Green -> good
        Colour.Red -> stop
        Colour.Neutral -> wait
    }
}

/** Two colours that are drawn one on the other, and the contrast ratio the pair must reach. */
class ColourPair(val name: String, val foreground: Int, val background: Int, val minimum: Double = Palette.MIN_CONTRAST)

object Palette {
    /** WCAG 2.x, level AA for normal text. */
    const val MIN_CONTRAST = 4.5

    const val WHITE = 0xFFFFFFFF.toInt()

    /** The brand orange: it fills the primary button and the logo, and nothing else. The same in both modes. */
    const val BRAND = 0xFFF7931A.toInt()

    /** The text on a brand fill, never white. */
    const val ON_BRAND = 0xFF111317.toInt()

    /** The outcome bands of a counter's sale, the same in both modes. */
    val paid = OutcomeBand(0xFF0E7350.toInt(), 0xFFDDF3EA.toInt())
    val waiting = OutcomeBand(0xFF1D4AA8.toInt(), 0xFFDCE7FD.toInt())
    val stopped = OutcomeBand(0xFFA9221B.toInt(), 0xFFFDE2E0.toInt())
    val ended = OutcomeBand(0xFF3D434C.toInt(), 0xFFD3D7DE.toInt())

    fun outcomes(): List<Pair<String, OutcomeBand>> = listOf("paid" to paid, "waiting" to waiting, "stop" to stopped, "ended" to ended)

    val light = Scheme(
        background = 0xFFF4F5F7.toInt(),
        paper = 0xFFFBFAF7.toInt(),
        surface = 0xFFFFFFFF.toInt(),
        text = 0xFF111317.toInt(),
        secondary = 0xFF4A505A.toInt(),
        hint = 0xFF6B7280.toInt(),
        divider = 0xFFE2E5EA.toInt(),
        fieldLine = 0xFFD5D9E0.toInt(),
        chip = 0xFFE7E9EE.toInt(),
        good = Tone(band = 0xFF12805C.toInt(), ink = 0xFF117E5B.toInt(), tint = 0xFFE7F6EF.toInt()),
        info = Tone(band = 0xFF1F4FB8.toInt(), ink = 0xFF1F4FB8.toInt(), tint = 0xFFEAF1FE.toInt()),
        wait = Tone(band = 0xFF5B616B.toInt(), ink = 0xFF5B616B.toInt(), tint = 0xFFECEEF1.toInt()),
        warn = Tone(band = 0xFF8A5300.toInt(), ink = 0xFF6D4200.toInt(), tint = 0xFFFFF8EE.toInt()),
        stop = Tone(band = 0xFFB3261E.toInt(), ink = 0xFFB3261E.toInt(), tint = 0xFFFDECEC.toInt()),
    )

    val dark = Scheme(
        background = 0xFF0E1013.toInt(),
        paper = 0xFF15140F.toInt(),
        surface = 0xFF171A1F.toInt(),
        text = 0xFFF2F4F7.toInt(),
        secondary = 0xFFA8B0BC.toInt(),
        hint = 0xFF8A93A0.toInt(),
        divider = 0xFF262A31.toInt(),
        fieldLine = 0xFF343A44.toInt(),
        chip = 0xFF22262D.toInt(),
        good = Tone(band = 0xFF1F7A55.toInt(), ink = 0xFF34C38F.toInt(), tint = 0xFF10291F.toInt()),
        info = Tone(band = 0xFF1B5DB0.toInt(), ink = 0xFF7FA8FF.toInt(), tint = 0xFF16233F.toInt()),
        wait = Tone(band = 0xFF4D545E.toInt(), ink = 0xFFA8B0BC.toInt(), tint = 0xFF22262D.toInt()),
        warn = Tone(band = 0xFF8A5300.toInt(), ink = 0xFFFFB74D.toInt(), tint = 0xFF2A2010.toInt()),
        stop = Tone(band = 0xFFB71C1C.toInt(), ink = 0xFFFF8A80.toInt(), tint = 0xFF3A1513.toInt()),
    )

    fun of(night: Boolean): Scheme = if (night) dark else light

    /** Every pair the apps draw: white on each band, and the text colours and inks on the background and on the surface. */
    fun pairs(s: Scheme): List<ColourPair> {
        val out = ArrayList<ColourPair>()
        for ((name, tone) in tones(s)) {
            out.add(ColourPair("white on the $name band", WHITE, tone.band))
            out.add(ColourPair("$name ink on the background", tone.ink, s.background))
            out.add(ColourPair("$name ink on the surface", tone.ink, s.surface))
        }
        out.add(ColourPair("text on the background", s.text, s.background))
        out.add(ColourPair("text on the surface", s.text, s.surface))
        out.add(ColourPair("secondary text on the background", s.secondary, s.background))
        out.add(ColourPair("secondary text on the surface", s.secondary, s.surface))
        out.add(ColourPair("hint on the surface", s.hint, s.surface))
        out.add(ColourPair("text on the paper", s.text, s.paper))
        out.add(ColourPair("secondary text on the paper", s.secondary, s.paper))
        out.add(ColourPair("hint on the paper", s.hint, s.paper))
        out.add(ColourPair("on-brand text on the brand fill", ON_BRAND, BRAND))
        for ((name, o) in outcomes()) {
            out.add(ColourPair("white on the $name outcome band", WHITE, o.band))
            out.add(ColourPair("the second line on the $name outcome band", o.sub, o.band))
        }
        for ((name, tone) in tones(s)) {
            out.add(ColourPair("$name ink on its tint", tone.ink, tone.tint))
            out.add(ColourPair("text on the $name tint", s.text, tone.tint))
        }
        return out
    }

    fun tones(s: Scheme): List<Pair<String, Tone>> =
        listOf("good" to s.good, "info" to s.info, "wait" to s.wait, "warn" to s.warn, "stop" to s.stop)

    /** The contrast ratio of two opaque colours, from 1 to 21 (WCAG 2.x relative luminance). */
    fun contrast(a: Int, b: Int): Double {
        val la = luminance(a)
        val lb = luminance(b)
        val hi = maxOf(la, lb)
        val lo = minOf(la, lb)
        return (hi + 0.05) / (lo + 0.05)
    }

    private fun luminance(argb: Int): Double {
        fun channel(shift: Int): Double {
            val c = ((argb shr shift) and 0xFF) / 255.0
            return if (c <= 0.03928) c / 12.92 else Math.pow((c + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    /** The colour darkened towards black by [part] (0 to 1), for a pressed button. */
    fun darker(argb: Int, part: Double): Int {
        fun channel(shift: Int): Int = (((argb shr shift) and 0xFF) * (1.0 - part)).toInt().coerceIn(0, 255) shl shift
        return (argb and 0xFF000000.toInt()) or channel(16) or channel(8) or channel(0)
    }

    /** The text of res/values/colors.xml (light) or res/values-night/colors.xml (dark): the resources the themes use. */
    fun colorsXml(night: Boolean): String {
        val s = of(night)
        val sb = StringBuilder()
        sb.append("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n")
        sb.append("<!-- Made from Palette.kt (apps/android-common/src/model); tests/PaletteTest.kt checks it. -->\n")
        sb.append("<resources>\n")
        fun item(name: String, argb: Int) {
            sb.append("    <color name=\"$name\">#${String.format("%08X", argb)}</color>\n")
        }
        item("background", s.background)
        item("surface", s.surface)
        item("text", s.text)
        item("text_secondary", s.secondary)
        item("text_hint", s.hint)
        item("divider", s.divider)
        item("field_line", s.fieldLine)
        item("chip", s.chip)
        item("brand", BRAND)
        item("on_brand", ON_BRAND)
        for ((name, tone) in tones(s)) {
            item("${name}_band", tone.band)
            item("${name}_ink", tone.ink)
            item("${name}_tint", tone.tint)
        }
        for ((name, o) in outcomes()) {
            item("outcome_$name", o.band)
            item("outcome_${name}_sub", o.sub)
        }
        sb.append("</resources>\n")
        return sb.toString()
    }
}

/** The sizes of text, in sp. The system sans font is used throughout; monospace only for codes, addresses, keys, invitations and amounts in a code. */
object Type {
    const val HEADING = 24f
    const val BODY = 17f
    const val SECONDARY = 14f

    /** The amount on a counter's sale screen. */
    const val AMOUNT = 44f

    /** The state of a sale in the band that fills the top third of its screen. */
    const val STATE = 34f

    /** Codes and quoted text, which are drawn in monospace. */
    const val CODE = 15f

    /** The bar's title and sub, the buttons, the fields, and the labels (the new look). */
    const val BAR_TITLE = 18f
    const val BAR_SUB = 13f
    const val FIELD = 17f
    const val LABEL = 13f
    const val LABELLED = 14f
}

/** The distances, in dp. */
object Space {
    const val GUTTER = 16
    const val BLOCK = 12
    const val TOUCH = 48
    const val BUTTON = 56
    const val FIELD = 56
    const val RADIUS = 12

    /** The new page: 20 dp gutter, 24 dp at the top and bottom, 18 dp between blocks, a 64 dp bar. */
    const val PAGE_GUTTER = 20
    const val PAGE_EDGE = 24
    const val PAGE_BLOCK = 18
    const val BAR = 64
    const val PRIMARY = 56
    const val SECONDARY = 52
    const val QUIET = 48
    const val BUTTON_RADIUS = 14
    const val FIELD_NEW = 52
}
