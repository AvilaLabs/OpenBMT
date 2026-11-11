package org.openbmt.app

import java.io.File

/**
 * The colours of the apps (src/model/Palette.kt): the contrast of every pair
 * that is drawn one on the other must reach 4.5 to 1 in the light scheme and
 * in the dark one, the five tones are told apart, and the resource files of
 * the themes are what the palette makes. They run on the desktop JVM, where no
 * Android class exists. The first argument is the apps/android-common
 * directory. OPENBMT_WRITE_COLOURS=1 writes the two resource files first.
 */
object PaletteTest {
    private var count = 0

    private fun ok(cond: Boolean, what: String) {
        count++
        if (!cond) throw AssertionError("FAILED: $what")
    }

    private fun contrastTests() {
        ok(Math.abs(Palette.contrast(0xFF000000.toInt(), Palette.WHITE) - 21.0) < 1e-9, "black on white is 21 to 1")
        ok(Math.abs(Palette.contrast(0xFF777777.toInt(), 0xFF777777.toInt()) - 1.0) < 1e-9, "a colour on itself is 1 to 1")
        ok(Palette.contrast(0xFF767676.toInt(), Palette.WHITE) in 4.5..4.6, "grey 767676 on white is the well-known 4.54")
        for ((name, night) in listOf("light" to false, "dark" to true)) {
            for (pair in Palette.pairs(Palette.of(night))) {
                val ratio = Palette.contrast(pair.foreground, pair.background)
                println(String.format("  %-5s %-36s %5.2f", name, pair.name, ratio))
                ok(ratio >= pair.minimum, "$name: ${pair.name} reaches ${pair.minimum} to 1 (is $ratio)")
            }
        }
    }

    /** A pressed button is the band darkened a little; white must still read on it. */
    private fun pressedTests() {
        for (night in listOf(false, true)) {
            for ((name, tone) in Palette.tones(Palette.of(night))) {
                val pressed = Palette.darker(tone.band, 0.15)
                ok(Palette.contrast(Palette.WHITE, pressed) >= Palette.contrast(Palette.WHITE, tone.band), "white on a pressed $name band reads at least as well")
            }
        }
    }

    private fun toneTests() {
        for (night in listOf(false, true)) {
            val s = Palette.of(night)
            val bands = Palette.tones(s).map { it.second.band }
            ok(bands.toSet().size == bands.size || night, "the bands are told apart (night=$night)")
            ok(s.of(Colour.Red).band == s.stop.band && s.of(Colour.Blue).band == s.info.band, "words colours map to tones")
            ok(s.of(Colour.Green).band == s.good.band && s.of(Colour.Neutral).band == s.wait.band, "green is good, neutral is wait")
            // DESIGN 2.6: waiting is blue or neutral, never red or amber.
            ok(s.wait.band != s.stop.band && s.wait.band != s.warn.band && s.info.band != s.stop.band && s.info.band != s.warn.band, "waiting is not the red or the amber")
        }
        ok(Colour.Red.argb == Palette.light.stop.band && Colour.Red.band(true) == Palette.dark.stop.band, "a colour has its light and dark band")
        ok(Type.HEADING == 24f && Type.BODY == 17f && Type.SECONDARY == 14f && Type.AMOUNT == 44f, "the sizes")
        ok(Space.PAGE_GUTTER == 20 && Space.PAGE_EDGE == 24 && Space.PAGE_BLOCK == 18 && Space.BAR == 64, "the distances of the new page")
        ok(Space.GUTTER == 16 && Space.BLOCK == 12 && Space.TOUCH == 48 && Space.BUTTON == 56, "the distances")
    }

    /** The new colours: brand, the outcome bands, the tints, and the names the pairs carry. */
    private fun newColourTests() {
        ok(Palette.BRAND == 0xFFF7931A.toInt() && Palette.ON_BRAND == 0xFF111317.toInt(), "brand and the text on it")
        ok(Palette.contrast(Palette.ON_BRAND, Palette.BRAND) >= Palette.MIN_CONTRAST, "on-brand text reads on the brand fill")
        ok(Palette.outcomes().map { it.first } == listOf("paid", "waiting", "stop", "ended"), "the four outcome bands")
        ok(Palette.paid.band == 0xFF0E7350.toInt() && Palette.waiting.band == 0xFF1D4AA8.toInt(), "paid and waiting bands")
        ok(Palette.stopped.band == 0xFFA9221B.toInt() && Palette.ended.band == 0xFF3D434C.toInt(), "stop and ended bands")
        ok(Palette.paid.sub == 0xFFDDF3EA.toInt() && Palette.ended.sub == 0xFFD3D7DE.toInt(), "the second lines")
        for (night in listOf(false, true)) {
            val s = Palette.of(night)
            for ((name, tone) in Palette.tones(s)) {
                ok(tone.tint != tone.band && tone.tint != s.background, "the $name tint is its own colour (night=$night)")
            }
            val names = Palette.pairs(s).map { it.name }
            ok(names.contains("good ink on its tint") && names.contains("text on the stop tint"), "the tint pairs are listed")
            ok(names.contains("white on the paid outcome band") && names.contains("the second line on the ended outcome band"), "the outcome pairs are listed")
        }
        ok(Palette.light.info.tint == 0xFFEAF1FE.toInt() && Palette.dark.warn.tint == 0xFF2A2010.toInt(), "tints of the table")
        ok(Palette.light.fieldLine == 0xFFD5D9E0.toInt() && Palette.dark.chip == 0xFF22262D.toInt(), "field line and chip")
    }

    private fun resourceTests(common: File) {
        for ((dir, night) in listOf("values" to false, "values-night" to true)) {
            val file = File(common, "res/$dir/colors.xml")
            if (System.getenv("OPENBMT_WRITE_COLOURS") == "1") {
                file.parentFile.mkdirs()
                file.writeText(Palette.colorsXml(night))
            }
            ok(file.isFile && file.readText() == Palette.colorsXml(night), "res/$dir/colors.xml is what the palette makes (run with OPENBMT_WRITE_COLOURS=1)")
        }
    }

    @JvmStatic
    fun main(args: Array<String>) {
        contrastTests()
        pressedTests()
        toneTests()
        newColourTests()
        resourceTests(File(args[0]))
        println("PaletteTest: $count checks passed")
    }
}
