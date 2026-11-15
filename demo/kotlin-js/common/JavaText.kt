@file:Suppress("PackageDirectoryMismatch", "unused")

package java.text

import java.util.Date

/** The one pattern the model writes, "yyyy-MM-dd HH:mm", on the page's local time. */
class SimpleDateFormat(private val pattern: String) {
    fun format(date: Date): String {
        require(pattern == "yyyy-MM-dd HH:mm") { "pattern $pattern is not supported" }
        val d: dynamic = js("new Date(0)")
        d.setTime(date.millis.toDouble())
        fun two(n: Int): String = n.toString().padStart(2, '0')
        val year = (d.getFullYear() as Int).toString().padStart(4, '0')
        return "$year-${two((d.getMonth() as Int) + 1)}-${two(d.getDate() as Int)} ${two(d.getHours() as Int)}:${two(d.getMinutes() as Int)}"
    }
}
