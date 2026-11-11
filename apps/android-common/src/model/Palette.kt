package org.openbmt.app

/**
 * The colours of the three apps, each with a light and a dark value, chosen
 * from the phone's night mode. No Android class is used, so the pairs that are
 * drawn on each other can be checked for contrast on the desktop JVM
 * (tests/PaletteTest.kt), and the resource files of the themes
 * (res/values/colors.xml and res/values-night/colors.xml) are made from here.
 *
 * A [Tone] has two values: the [band], a fill that carries white text, and the
 * [ink], the same tone as text or a line on the page's own background.
 */
class Tone(val band: Int, val ink: Int)

/** One set of colours: either the light one or the dark one. */
class Scheme(
    val background: Int,
    val surface: Int,
    val text: Int,
    val secondary: Int,
    val hint: Int,
    val divider: Int,
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

    val light = Scheme(
        background = 0xFFFFFFFF.toInt(),
        surface = 0xFFF1F4F6.toInt(),
        text = 0xFF15191C.toInt(),
        secondary = 0xFF46565F.toInt(),
        hint = 0xFF5F6F78.toInt(),
        divider = 0xFFCFD8DC.toInt(),
        good = Tone(band = 0xFF2E7D32.toInt(), ink = 0xFF1B5E20.toInt()),
        info = Tone(band = 0xFF1565C0.toInt(), ink = 0xFF0D47A1.toInt()),
        wait = Tone(band = 0xFF546E7A.toInt(), ink = 0xFF455A64.toInt()),
        warn = Tone(band = 0xFF8A5300.toInt(), ink = 0xFF6D4200.toInt()),
        stop = Tone(band = 0xFFC62828.toInt(), ink = 0xFFB71C1C.toInt()),
    )

    val dark = Scheme(
        background = 0xFF121417.toInt(),
        surface = 0xFF1E2327.toInt(),
        text = 0xFFECEFF1.toInt(),
        secondary = 0xFFB0BEC5.toInt(),
        hint = 0xFF90A4AE.toInt(),
        divider = 0xFF37474F.toInt(),
        good = Tone(band = 0xFF2A7430.toInt(), ink = 0xFF81C784.toInt()),
        info = Tone(band = 0xFF1B5DB0.toInt(), ink = 0xFF90CAF9.toInt()),
        wait = Tone(band = 0xFF4D6772.toInt(), ink = 0xFFB0BEC5.toInt()),
        warn = Tone(band = 0xFF8A5300.toInt(), ink = 0xFFFFB74D.toInt()),
        stop = Tone(band = 0xFFB71C1C.toInt(), ink = 0xFFEF9A9A.toInt()),
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
        item("divider", s.divider)
        for ((name, tone) in tones(s)) {
            item("${name}_band", tone.band)
            item("${name}_ink", tone.ink)
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
}

/** The distances, in dp. */
object Space {
    const val GUTTER = 16
    const val BLOCK = 12
    const val TOUCH = 48
    const val BUTTON = 56
    const val FIELD = 56
    const val RADIUS = 12
}
