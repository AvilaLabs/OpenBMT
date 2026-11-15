@file:Suppress("PackageDirectoryMismatch", "unused")

package java.time

/** The page's own time zone; the one the model asks for is the system's. */
class ZoneId private constructor() {
    companion object {
        fun systemDefault(): ZoneId = ZoneId()
    }
}

class LocalDate internal constructor(private val epochSecondsOfMidnight: Long) {
    fun atStartOfDay(zone: ZoneId): ZonedDateTime = ZonedDateTime(epochSecondsOfMidnight)
}

class ZonedDateTime internal constructor(private val epochSeconds: Long) {
    fun toLocalDate(): LocalDate {
        val d: dynamic = js("new Date(0)")
        d.setTime(epochSeconds.toDouble() * 1000.0)
        d.setHours(0, 0, 0, 0)
        return LocalDate(((d.getTime() as Double) / 1000.0).toLong())
    }
    fun toEpochSecond(): Long = epochSeconds
}

class Instant private constructor(private val epochSeconds: Long) {
    fun atZone(zone: ZoneId): ZonedDateTime = ZonedDateTime(epochSeconds)

    companion object {
        fun ofEpochSecond(seconds: Long): Instant = Instant(seconds)
    }
}
