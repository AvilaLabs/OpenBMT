package org.openbmt.checkout

import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.security.MessageDigest
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/** JVM-only checks for the sealed file/CAS layer. The production Keystore cipher is Android-only. */
object SealedCheckoutAuthorityStorageTest {
    fun run() {
        compareAndSwapIsSerializedAndScopeBound()
        malformedAndUnauthenticatedFilesFailClosed()
        failedSealLeavesPriorRevisionIntact()
    }

    private fun compareAndSwapIsSerializedAndScopeBound() {
        val directory = Files.createTempDirectory("checkout-authority-sealed-").toFile()
        val path = File(directory, "checkout-authority.obks")
        val lockPath = File(directory, "checkout-authority.obks.lock")
        val sealer = FakeSealer()
        try {
            val first = SealedCheckoutAuthorityStorage(directory, SCOPE, sealer)
            val second = SealedCheckoutAuthorityStorage(directory, SCOPE, sealer)
            check(first.read() == null)
            check(first.compareAndSwap(null, byteArrayOf(1, 2, 3)))
            check(!second.compareAndSwap(null, byteArrayOf(9)))
            check(second.read()!!.contentEquals(byteArrayOf(1, 2, 3)))
            check(second.compareAndSwap(byteArrayOf(1, 2, 3), byteArrayOf(4, 5)))
            check(first.read()!!.contentEquals(byteArrayOf(4, 5)))

            val start = CountDownLatch(1)
            val failure = AtomicReference<Throwable?>(null)
            val outcomes = BooleanArray(2)
            val stores = listOf(first, second)
            val threads = stores.mapIndexed { index, store ->
                Thread {
                    try {
                        start.await()
                        outcomes[index] = store.compareAndSwap(byteArrayOf(4, 5), byteArrayOf((7 + index).toByte()))
                    } catch (error: Throwable) {
                        failure.compareAndSet(null, error)
                    }
                }
            }
            threads.forEach(Thread::start)
            start.countDown()
            threads.forEach(Thread::join)
            check(failure.get() == null && outcomes.count { it } == 1)

            val otherScope = SCOPE.copy(deviceId = "device-other")
            val moved = SealedCheckoutAuthorityStorage(directory, otherScope, sealer)
            check(runCatching { moved.read() }.exceptionOrNull() is IOException)
        } finally {
            path.delete()
            lockPath.delete()
            directory.delete()
        }
    }

    private fun malformedAndUnauthenticatedFilesFailClosed() {
        val directory = Files.createTempDirectory("checkout-authority-corrupt-").toFile()
        val file = File(directory, "checkout-authority.obks")
        try {
            val storage = SealedCheckoutAuthorityStorage(directory, SCOPE, FakeSealer())
            check(storage.compareAndSwap(null, byteArrayOf(8, 9, 10)))
            val valid = file.readBytes()

            file.writeBytes(valid.copyOf().also { it[4] = 99 }) // unsupported envelope version
            check(runCatching { storage.read() }.exceptionOrNull() is IOException)

            file.writeBytes(valid.copyOf().also { it[it.lastIndex] = (it.last().toInt() xor 1).toByte() })
            check(runCatching { storage.read() }.exceptionOrNull() is IOException)

            file.writeBytes(byteArrayOf(1, 2, 3, 4))
            check(runCatching { storage.read() }.exceptionOrNull() is IOException)
        } finally {
            file.delete()
            File(directory, "checkout-authority.obks.lock").delete()
            directory.delete()
        }
    }

    private fun failedSealLeavesPriorRevisionIntact() {
        val directory = Files.createTempDirectory("checkout-authority-seal-failure-").toFile()
        try {
            val sealer = FakeSealer()
            val storage = SealedCheckoutAuthorityStorage(directory, SCOPE, sealer)
            check(storage.compareAndSwap(null, byteArrayOf(1)))
            sealer.failNextSeal = true
            check(runCatching { storage.compareAndSwap(byteArrayOf(1), byteArrayOf(2)) }.isFailure)
            check(storage.read()!!.contentEquals(byteArrayOf(1)))
        } finally {
            File(directory, "checkout-authority.obks").delete()
            File(directory, "checkout-authority.obks.lock").delete()
            directory.delete()
        }
    }

    private class FakeSealer : CheckoutAuthorityRecordSealer {
        private val counter = AtomicInteger()
        var failNextSeal = false

        override fun seal(plain: ByteArray, aad: ByteArray): CheckoutAuthoritySealedValue {
            if (failNextSeal) {
                failNextSeal = false
                throw IOException("test seal failure")
            }
            val iv = ByteArray(12).also { bytes ->
                val value = counter.incrementAndGet()
                for (index in bytes.indices) bytes[index] = (value + index).toByte()
            }
            val tag = digest(aad, iv, plain)
            return CheckoutAuthoritySealedValue(iv, plain + tag)
        }

        override fun open(sealed: CheckoutAuthoritySealedValue, aad: ByteArray): ByteArray {
            if (sealed.ciphertext.size < 32) throw IOException("test authentication failed")
            val plain = sealed.ciphertext.copyOfRange(0, sealed.ciphertext.size - 32)
            val tag = sealed.ciphertext.copyOfRange(sealed.ciphertext.size - 32, sealed.ciphertext.size)
            if (!MessageDigest.isEqual(tag, digest(aad, sealed.iv, plain))) throw IOException("test authentication failed")
            return plain
        }

        private fun digest(aad: ByteArray, iv: ByteArray, plain: ByteArray): ByteArray =
            MessageDigest.getInstance("SHA-256").digest(aad + iv + plain)
    }

    private val SCOPE = CheckoutAuthorityScope("merchant-main", "store-main", "location-main", "register-main", "device-main")
}
