@file:Suppress("PackageDirectoryMismatch", "unused")

package java.time.format

/** A text that is not a date, as the JVM throws it. */
class DateTimeParseException(message: String? = null) : java.time.DateTimeException(message)
