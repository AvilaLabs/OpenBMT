@file:Suppress("PackageDirectoryMismatch", "unused")

package java.time

/** What the calendar calls do not accept, as the JVM throws it. */
open class DateTimeException(message: String? = null) : RuntimeException(message)

/**
 * A time zone: the page's own (the one the model asks for is the system's) or a fixed offset from UTC.
 * [offsetSeconds] is the offset at an instant, [startOfDay] the first second of a day in the zone.
 */
open class ZoneId internal constructor() {
    internal open fun offsetSeconds(epochSeconds: Long): Int {
        val d: dynamic = js("new Date(0)")
        d.setTime(epochSeconds.toDouble() * 1000.0)
        return -(d.getTimezoneOffset() as Int) * 60
    }

    internal open fun startOfDay(day: LocalDate): Long {
        val d: dynamic = js("new Date(0)")
        d.setFullYear(day.year, day.monthValue - 1, day.dayOfMonth)
        d.setHours(0, 0, 0, 0)
        return ((d.getTime() as Double) / 1000.0).toLong()
    }

    companion object {
        fun systemDefault(): ZoneId = ZoneId()
    }
}

/** A fixed offset from UTC. */
class ZoneOffset private constructor(private val total: Int) : ZoneId() {
    override fun offsetSeconds(epochSeconds: Long): Int = total
    override fun startOfDay(day: LocalDate): Long = day.toEpochDay() * 86_400L - total

    companion object {
        val UTC = ZoneOffset(0)

        fun ofTotalSeconds(seconds: Int): ZoneOffset {
            if (seconds < -64_800 || seconds > 64_800) throw DateTimeException("the offset is not within 18 hours")
            return if (seconds == 0) UTC else ZoneOffset(seconds)
        }
    }
}

/** A day of the calendar (proleptic Gregorian), kept as its number of days from 1970-01-01. */
class LocalDate private constructor(private val epochDay: Long) : Comparable<LocalDate> {
    val year: Int get() = civil()[0].toInt()
    val monthValue: Int get() = civil()[1].toInt()
    val dayOfMonth: Int get() = civil()[2].toInt()

    private fun civil(): LongArray {
        val z = epochDay + 719_468
        val era = z.floorDiv(146_097L)
        val doe = z - era * 146_097
        val yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365
        val doy = doe - (365 * yoe + yoe / 4 - yoe / 100)
        val mp = (5 * doy + 2) / 153
        val d = doy - (153 * mp + 2) / 5 + 1
        val m = if (mp < 10) mp + 3 else mp - 9
        val y = yoe + era * 400 + (if (m <= 2) 1 else 0)
        return longArrayOf(y, m, d)
    }

    fun toEpochDay(): Long = epochDay
    fun plusDays(days: Long): LocalDate = LocalDate(epochDay + days)
    fun plusMonths(months: Long): LocalDate {
        val total = year.toLong() * 12 + (monthValue - 1) + months
        val y = total.floorDiv(12L)
        val m = (total - y * 12).toInt() + 1
        return of(y.toInt(), m, minOf(dayOfMonth, lengthOfMonth(y.toInt(), m)))
    }
    fun isBefore(other: LocalDate): Boolean = epochDay < other.epochDay
    fun isAfter(other: LocalDate): Boolean = epochDay > other.epochDay
    override fun compareTo(other: LocalDate): Int = epochDay.compareTo(other.epochDay)
    override fun equals(other: Any?): Boolean = other is LocalDate && other.epochDay == epochDay
    override fun hashCode(): Int = epochDay.hashCode()

    /** The first second of the day in [zone]. */
    fun atStartOfDay(zone: ZoneId): ZonedDateTime = ZonedDateTime(zone.startOfDay(this), zone)
    fun atStartOfDay(): LocalDateTime = LocalDateTime(epochDay * 86_400L)

    /** "2026-12-01". */
    override fun toString(): String {
        val c = civil()
        fun two(n: Long): String = n.toString().padStart(2, '0')
        return "${c[0].toString().padStart(4, '0')}-${two(c[1])}-${two(c[2])}"
    }

    companion object {
        private fun lengthOfMonth(year: Int, month: Int): Int = when (month) {
            2 -> if (year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)) 29 else 28
            4, 6, 9, 11 -> 30
            else -> 31
        }

        fun of(year: Int, month: Int, day: Int): LocalDate {
            if (month !in 1..12) throw DateTimeException("the month $month is not of the calendar")
            if (day !in 1..lengthOfMonth(year, month)) throw DateTimeException("the day $day is not of the month")
            val y = if (month <= 2) year - 1L else year.toLong()
            val era = y.floorDiv(400L)
            val yoe = y - era * 400
            val mp = if (month > 2) month - 3 else month + 9
            val doy = (153 * mp + 2) / 5 + day - 1
            val doe = yoe * 365 + yoe / 4 - yoe / 100 + doy
            return LocalDate(era * 146_097 + doe - 719_468)
        }

        fun ofEpochDay(day: Long): LocalDate = LocalDate(day)

        /** The ISO form "yyyy-mm-dd" and no other. */
        fun parse(text: CharSequence): LocalDate {
            val m = Regex("(\\d{4})-(\\d{2})-(\\d{2})").matchEntire(text) ?: throw java.time.format.DateTimeParseException("$text does not read as a date")
            return try {
                of(m.groupValues[1].toInt(), m.groupValues[2].toInt(), m.groupValues[3].toInt())
            } catch (e: DateTimeException) {
                throw java.time.format.DateTimeParseException(e.message)
            }
        }
    }
}

/** A day and a time of it with no zone. */
class LocalDateTime internal constructor(private val seconds: Long) {
    /** The seconds from 1970-01-01T00:00 UTC if the time is at [offset]. */
    fun toEpochSecond(offset: ZoneOffset): Long = seconds - offset.offsetSeconds(0)
}

class ZonedDateTime internal constructor(private val epochSeconds: Long, private val zone: ZoneId) {
    fun toLocalDate(): LocalDate =
        LocalDate.ofEpochDay((epochSeconds + zone.offsetSeconds(epochSeconds)).floorDiv(86_400L))
    fun toEpochSecond(): Long = epochSeconds
}

/** A time and the offset it is read at. */
class OffsetDateTime internal constructor(private val epochSeconds: Long, private val offset: ZoneOffset) {
    fun toLocalDate(): LocalDate =
        LocalDate.ofEpochDay((epochSeconds + offset.offsetSeconds(epochSeconds)).floorDiv(86_400L))
}

class Instant private constructor(private val epochSeconds: Long) {
    fun atZone(zone: ZoneId): ZonedDateTime = ZonedDateTime(epochSeconds, zone)
    fun atOffset(offset: ZoneOffset): OffsetDateTime = OffsetDateTime(epochSeconds, offset)

    companion object {
        fun ofEpochSecond(seconds: Long): Instant = Instant(seconds)
    }
}
