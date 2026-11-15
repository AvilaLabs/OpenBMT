// java.math for the browser build (see JavaIo.kt): the whole numbers of any
// size, only for the calls the model makes (a product that is past 64 bits,
// divided back down).
@file:Suppress("PackageDirectoryMismatch", "unused")

package java.math

/** A whole number of any size: a sign and the digits of the size, in base 65536, the least first. */
class BigInteger private constructor(private val sign: Int, private val mag: IntArray) {
    fun signum(): Int = sign

    /** The count of bits of the size, as the JVM counts them for a positive number. */
    fun bitLength(): Int {
        if (mag.isEmpty()) return 0
        return (mag.size - 1) * 16 + (32 - mag.last().countLeadingZeroBits())
    }

    /** The low 64 bits, as the JVM takes them. */
    fun toLong(): Long {
        var v = 0L
        for (i in minOf(mag.size, 4) - 1 downTo 0) v = (v shl 16) or mag[i].toLong()
        return if (sign < 0) -v else v
    }

    fun multiply(other: BigInteger): BigInteger {
        if (sign == 0 || other.sign == 0) return ZERO
        val out = IntArray(mag.size + other.mag.size)
        for (i in mag.indices) {
            var carry = 0L
            for (j in other.mag.indices) {
                val cur = out[i + j].toLong() + mag[i].toLong() * other.mag[j].toLong() + carry
                out[i + j] = (cur and 0xffff).toInt()
                carry = cur ushr 16
            }
            out[i + other.mag.size] += carry.toInt()
        }
        return make(sign * other.sign, out)
    }

    /** The quotient, cut towards zero. */
    fun divide(other: BigInteger): BigInteger {
        if (other.sign == 0) throw ArithmeticException("division by zero")
        if (compareMag(mag, other.mag) < 0) return ZERO
        val quotient = IntArray(mag.size)
        var rest = IntArray(0)
        for (bit in mag.size * 16 - 1 downTo 0) {
            rest = shiftLeftOne(rest, (mag[bit / 16] shr (bit % 16)) and 1)
            if (compareMag(rest, other.mag) >= 0) {
                rest = subtractMag(rest, other.mag)
                quotient[bit / 16] = quotient[bit / 16] or (1 shl (bit % 16))
            }
        }
        return make(sign * other.sign, quotient)
    }

    override fun toString(): String {
        if (sign == 0) return "0"
        val ten = valueOf(10)
        var n = BigInteger(1, mag)
        val digits = StringBuilder()
        while (n.sign != 0) {
            val q = n.divide(ten)
            digits.append('0' + n.subtract(q.multiply(ten)).toLong().toInt())
            n = q
        }
        return (if (sign < 0) "-" else "") + digits.reverse()
    }

    private fun subtract(other: BigInteger): BigInteger = make(1, subtractMag(mag, other.mag))

    companion object {
        val ZERO = BigInteger(0, IntArray(0))

        fun valueOf(value: Long): BigInteger {
            if (value == 0L) return ZERO
            // The size of Long.MIN_VALUE is 2^63, which does not negate in a Long: read it as an unsigned number.
            var m = if (value < 0) -value else value
            val out = IntArray(4)
            for (i in 0 until 4) {
                out[i] = (m and 0xffff).toInt()
                m = m ushr 16
            }
            return make(if (value < 0) -1 else 1, out)
        }

        private fun make(sign: Int, digits: IntArray): BigInteger {
            var n = digits.size
            while (n > 0 && digits[n - 1] == 0) n--
            return if (n == 0) ZERO else BigInteger(sign, digits.copyOf(n))
        }

        private fun compareMag(a: IntArray, b: IntArray): Int {
            if (a.size != b.size) return a.size.compareTo(b.size)
            for (i in a.indices.reversed()) if (a[i] != b[i]) return a[i].compareTo(b[i])
            return 0
        }

        private fun shiftLeftOne(a: IntArray, low: Int): IntArray {
            val out = IntArray(a.size + 1)
            var carry = low
            for (i in a.indices) {
                out[i] = ((a[i] shl 1) or carry) and 0xffff
                carry = a[i] shr 15
            }
            out[a.size] = carry
            var n = out.size
            while (n > 0 && out[n - 1] == 0) n--
            return out.copyOf(n)
        }

        /** [a] minus [b], where [a] is not the smaller. */
        private fun subtractMag(a: IntArray, b: IntArray): IntArray {
            val out = IntArray(a.size)
            var borrow = 0
            for (i in a.indices) {
                var d = a[i] - borrow - (if (i < b.size) b[i] else 0)
                borrow = if (d < 0) 1 else 0
                if (d < 0) d += 0x10000
                out[i] = d
            }
            var n = out.size
            while (n > 0 && out[n - 1] == 0) n--
            return out.copyOf(n)
        }
    }
}
