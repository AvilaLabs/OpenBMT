package org.openbmt.checkout

import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.RandomAccessFile
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption
import java.util.concurrent.ConcurrentHashMap

/** Explicit caller assertion that platform install checks found no prior app or business state. */
class CheckoutFreshInstallEvidence(installToken: String) {
    internal val installToken = installToken

    init {
        require(installToken.length in 16..128 && installToken.all { it.isLetterOrDigit() || it in "-_" }) {
            "Invalid fresh-install evidence"
        }
    }

    override fun toString(): String = "CheckoutFreshInstallEvidence(redacted)"
}

/** Durable installation sentinel, normally backed by a dedicated Android Keystore key alias. */
interface CheckoutInstallationMarker {
    fun isEstablished(): Boolean

    /** True once a descriptor has been durably installed at least once. */
    fun isDescriptorCommitted(): Boolean

    /** True only for an interrupted first creation, never for an older established installation. */
    fun canResumeInitialCreation(): Boolean

    /** Creates the marker only when absent; returns false if it already exists. */
    fun establish(): Boolean

    /** Durable one-way marker written after the first atomic descriptor commit. */
    fun commitDescriptor()

    /** Authenticates public descriptor bytes using a key that does not leave the platform keystore. */
    fun authenticate(bytes: ByteArray): ByteArray

    fun verify(bytes: ByteArray, authenticator: ByteArray): Boolean
}

enum class CheckoutProvisioningState { PENDING_ENROLLMENT, COMPLETED }

data class CheckoutInstallationDescriptor(
    val scope: CheckoutAuthorityScope,
    val provisioningState: CheckoutProvisioningState,
)

/**
 * Strict, public descriptor for the exact authority installation scope. The file is intentionally
 * unencrypted so recovery tooling can identify the configured merchant/register; integrity comes
 * from a strict codec plus a Keystore HMAC and durable platform marker. Authority records remain sealed.
 * This class never creates a scope while loading an absent descriptor.
 */
class CheckoutInstallationScopeStore(
    directory: File,
    private val marker: CheckoutInstallationMarker,
    name: String = DEFAULT_NAME,
) {
    private val directory = directory.absoluteFile
    private val safeName = validateName(name)
    private val file = File(this.directory, safeName)
    private val lockFile = File(this.directory, "$safeName.lock")
    private val processLock = PROCESS_LOCKS.computeIfAbsent(file.canonicalPath) { Any() }

    /** Reads the persisted scope. Absence, marker loss, corruption, and unsupported versions fail. */
    fun load(expectedScope: CheckoutAuthorityScope? = null): CheckoutAuthorityScope =
        loadDescriptor(expectedScope).scope

    fun loadDescriptor(expectedScope: CheckoutAuthorityScope? = null): CheckoutInstallationDescriptor = withLock {
        if (!file.exists()) throw IOException("Checkout installation scope is unavailable")
        if (!marker.isEstablished()) throw IOException("Checkout installation marker is unavailable")
        val envelope = readBounded()
        val payloadSize = envelope.size - AUTHENTICATOR_BYTES - 2
        if (payloadSize !in MIN_V1_PAYLOAD_BYTES..MAX_PAYLOAD_BYTES) {
            throw IOException("Checkout installation scope has an invalid size")
        }
        val payload = envelope.copyOfRange(0, payloadSize)
        val authSize = ((envelope[payloadSize].toInt() and 0xff) shl 8) or
            (envelope[payloadSize + 1].toInt() and 0xff)
        val authenticator = envelope.copyOfRange(payloadSize + 2, envelope.size)
        if (authSize != AUTHENTICATOR_BYTES || !marker.verify(payload, authenticator)) {
            throw IOException("Checkout installation scope authentication failed")
        }
        val descriptor = decode(payload)
        if (expectedScope != null && descriptor.scope != expectedScope) {
            throw IOException("Checkout installation scope does not match")
        }
        if (!marker.isDescriptorCommitted()) marker.commitDescriptor()
        if (payload[4].toInt() == VERSION_V1) writeDescriptor(descriptor)
        descriptor
    }

    /**
     * Creates the descriptor exactly once. The scope factory runs only after explicit fresh-install
     * evidence and under the cross-process lock, so it cannot mint replacement IDs on reload.
     */
    fun createOnFreshInstall(
        evidence: CheckoutFreshInstallEvidence,
        createScope: () -> CheckoutAuthorityScope,
    ): CheckoutAuthorityScope = createDescriptorOnFreshInstall(evidence, createScope).scope

    fun createDescriptorOnFreshInstall(
        evidence: CheckoutFreshInstallEvidence,
        createScope: () -> CheckoutAuthorityScope,
    ): CheckoutInstallationDescriptor = withLock {
        if (evidence.installToken.length !in 16..128) throw IOException("Fresh-install evidence is invalid")
        if (file.exists() || marker.isDescriptorCommitted()) {
            throw IOException("Checkout installation scope already exists")
        }
        if (marker.isEstablished()) {
            if (!marker.canResumeInitialCreation()) throw IOException("Checkout installation marker already exists")
        } else if (!marker.establish()) {
            throw IOException("Checkout installation marker already exists")
        }
        val scope = try {
            createScope()
        } catch (_: Exception) {
            throw IOException("Checkout installation scope could not be created")
        }
        val descriptor = CheckoutInstallationDescriptor(scope, CheckoutProvisioningState.PENDING_ENROLLMENT)
        val payload = encode(descriptor)
        val authenticator = try { marker.authenticate(payload) } catch (_: Exception) {
            payload.fill(0)
            throw IOException("Checkout installation scope could not be authenticated")
        }
        if (authenticator.size != AUTHENTICATOR_BYTES) {
            payload.fill(0)
            authenticator.fill(0)
            throw IOException("Checkout installation authenticator is invalid")
        }
        val encoded = ByteArrayOutputStream(payload.size + AUTHENTICATOR_BYTES + 2).also { bytes ->
            DataOutputStream(bytes).use { data ->
                data.write(payload)
                data.writeShort(authenticator.size)
                data.write(authenticator)
            }
        }.toByteArray()
        try {
            writeCreateOnce(encoded)
            marker.commitDescriptor()
        } finally {
            payload.fill(0)
            authenticator.fill(0)
            encoded.fill(0)
        }
        descriptor
    }

    /** One-way transition after the authority store confirms its manager record is durable. */
    fun markCompleted(
        expectedScope: CheckoutAuthorityScope,
        authority: CheckoutAuthority,
    ): CheckoutInstallationDescriptor = withLock {
        val current = loadDescriptorUnlocked(expectedScope)
        val profile = authority.loginProfile() ?: throw IOException("Manager authority record is unavailable")
        if (profile.scope != expectedScope || profile.enabledStaff.none { it.role == CheckoutAuthorityRole.MANAGER }) {
            throw IOException("Manager authority record is not confirmed")
        }
        if (current.provisioningState == CheckoutProvisioningState.COMPLETED) return@withLock current
        val completed = current.copy(provisioningState = CheckoutProvisioningState.COMPLETED)
        writeDescriptor(completed)
        completed
    }

    /** Refuses re-enrollment if a completed installation lost its authority record. */
    fun validateAuthorityRecord(authority: CheckoutAuthority): CheckoutAuthorityLoginProfile? = withLock {
        val descriptor = loadDescriptorUnlocked()
        val profile = authority.loginProfile()
        if (profile == null && descriptor.provisioningState == CheckoutProvisioningState.COMPLETED) {
            throw CheckoutAuthorityUnavailableException()
        }
        if (profile != null && (profile.scope != descriptor.scope ||
                profile.enabledStaff.none { it.role == CheckoutAuthorityRole.MANAGER })) {
            throw CheckoutAuthorityUnavailableException()
        }
        profile
    }

    private fun loadDescriptorUnlocked(expectedScope: CheckoutAuthorityScope? = null): CheckoutInstallationDescriptor {
        if (!file.exists() || !marker.isEstablished() || !marker.isDescriptorCommitted()) {
            throw IOException("Checkout installation scope is unavailable")
        }
        val envelope = readBounded()
        val payloadSize = envelope.size - AUTHENTICATOR_BYTES - 2
        if (payloadSize !in MIN_V1_PAYLOAD_BYTES..MAX_PAYLOAD_BYTES) {
            throw IOException("Checkout installation scope has an invalid size")
        }
        val payload = envelope.copyOfRange(0, payloadSize)
        val authSize = ((envelope[payloadSize].toInt() and 0xff) shl 8) or
            (envelope[payloadSize + 1].toInt() and 0xff)
        val authenticator = envelope.copyOfRange(payloadSize + 2, envelope.size)
        if (authSize != AUTHENTICATOR_BYTES || !marker.verify(payload, authenticator)) {
            throw IOException("Checkout installation scope authentication failed")
        }
        val descriptor = decode(payload)
        if (expectedScope != null && descriptor.scope != expectedScope) {
            throw IOException("Checkout installation scope does not match")
        }
        if (payload[4].toInt() == VERSION_V1) writeDescriptor(descriptor)
        return descriptor
    }

    private fun writeDescriptor(descriptor: CheckoutInstallationDescriptor) {
        val payload = encode(descriptor)
        val authenticator = try { marker.authenticate(payload) } catch (_: Exception) {
            payload.fill(0)
            throw IOException("Checkout installation scope could not be authenticated")
        }
        if (authenticator.size != AUTHENTICATOR_BYTES) {
            payload.fill(0)
            authenticator.fill(0)
            throw IOException("Checkout installation authenticator is invalid")
        }
        val encoded = ByteArrayOutputStream(payload.size + AUTHENTICATOR_BYTES + 2).also { bytes ->
            DataOutputStream(bytes).use { data ->
                data.write(payload)
                data.writeShort(authenticator.size)
                data.write(authenticator)
            }
        }.toByteArray()
        try {
            writeAtomic(encoded, replace = true)
            marker.commitDescriptor()
        } finally {
            payload.fill(0)
            authenticator.fill(0)
            encoded.fill(0)
        }
    }

    private fun readBounded(): ByteArray {
        val size = file.length()
        if (size !in MIN_ENVELOPE_BYTES.toLong()..MAX_BYTES.toLong()) {
            throw IOException("Checkout installation scope has an invalid size")
        }
        val bytes = try { Files.readAllBytes(file.toPath()) } catch (_: Exception) {
            throw IOException("Checkout installation scope could not be read")
        }
        if (bytes.size.toLong() != size) throw IOException("Checkout installation scope changed while reading")
        return bytes
    }

    private fun writeCreateOnce(bytes: ByteArray) {
        writeAtomic(bytes, replace = false)
    }

    private fun writeAtomic(bytes: ByteArray, replace: Boolean) {
        var temp: File? = null
        try {
            temp = File.createTempFile(".$safeName.", ".tmp", directory)
            FileOutputStream(temp).use { output ->
                output.write(bytes)
                output.flush()
                output.fd.sync()
            }
            if (replace) {
                Files.move(temp.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
            } else {
                Files.move(temp.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE)
            }
            temp = null
            try { FileChannel.open(directory.toPath(), StandardOpenOption.READ).use { it.force(true) } }
            catch (_: Exception) { }
        } catch (error: IOException) {
            throw error
        } catch (_: Exception) {
            throw IOException("Checkout installation scope could not be committed")
        } finally {
            temp?.delete()
        }
    }

    private fun <T> withLock(block: () -> T): T = synchronized(processLock) {
        if (!directory.exists() && !directory.mkdirs()) throw IOException("Checkout installation directory unavailable")
        if (!directory.isDirectory) throw IOException("Checkout installation directory unavailable")
        try {
            RandomAccessFile(lockFile, "rw").use { lock ->
                lock.channel.use { channel -> channel.lock().use { block() } }
            }
        } catch (error: CheckoutAuthorityUnavailableException) {
            throw error
        } catch (error: IOException) {
            throw error
        } catch (_: Exception) {
            throw IOException("Checkout installation scope storage unavailable")
        }
    }

    override fun toString(): String = "CheckoutInstallationScopeStore(redacted)"

    private companion object {
        const val DEFAULT_NAME = "checkout-installation-scope.obis"
        const val FORMAT_VERSION = 2
        const val VERSION_V1 = 1
        const val MAX_PART_BYTES = 64
        const val AUTHENTICATOR_BYTES = 32
        const val MAX_PAYLOAD_BYTES = 4 + 1 + 1 + 5 * (2 + MAX_PART_BYTES)
        const val MIN_V1_PAYLOAD_BYTES = 4 + 1 + 5 * 3
        const val MIN_V2_PAYLOAD_BYTES = MIN_V1_PAYLOAD_BYTES + 1
        const val MAX_V1_PAYLOAD_BYTES = MAX_PAYLOAD_BYTES - 1
        const val MAX_BYTES = MAX_PAYLOAD_BYTES + 2 + AUTHENTICATOR_BYTES
        const val MIN_ENVELOPE_BYTES = MIN_V1_PAYLOAD_BYTES + 2 + AUTHENTICATOR_BYTES
        val MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'I'.code.toByte(), 'S'.code.toByte())
        val PROCESS_LOCKS = ConcurrentHashMap<String, Any>()

        fun validateName(name: String): String {
            if (name.isEmpty() || name.length > 96 || name == "." || name == ".." ||
                name.any { !(it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '.' || it == '_' || it == '-') }) {
                throw IllegalArgumentException("Invalid checkout installation descriptor name")
            }
            return name
        }

        fun encode(descriptor: CheckoutInstallationDescriptor): ByteArray {
            val out = ByteArrayOutputStream()
            DataOutputStream(out).use { data ->
                data.write(MAGIC)
                data.writeByte(FORMAT_VERSION)
                data.writeByte(descriptor.provisioningState.ordinal)
                val scope = descriptor.scope
                listOf(scope.merchantId, scope.storeId, scope.locationId, scope.registerId, scope.deviceId).forEach {
                    val bytes = it.toByteArray(Charsets.UTF_8)
                    if (bytes.isEmpty() || bytes.size > MAX_PART_BYTES) throw IOException("Checkout installation scope is too large")
                    data.writeShort(bytes.size)
                    data.write(bytes)
                }
            }
            return out.toByteArray()
        }

        fun decode(bytes: ByteArray): CheckoutInstallationDescriptor {
            if (bytes.size !in MIN_V1_PAYLOAD_BYTES..MAX_PAYLOAD_BYTES) {
                throw IOException("Checkout installation scope has an invalid size")
            }
            try {
                DataInputStream(bytes.inputStream()).use { data ->
                    val magic = ByteArray(MAGIC.size).also(data::readFully)
                    if (!magic.contentEquals(MAGIC)) {
                        throw IOException("Checkout installation scope format is unsupported")
                    }
                    val version = data.readUnsignedByte()
                    val provisioningState = when (version) {
                        VERSION_V1 -> CheckoutProvisioningState.PENDING_ENROLLMENT.also {
                            if (bytes.size !in MIN_V1_PAYLOAD_BYTES..MAX_V1_PAYLOAD_BYTES) {
                                throw IOException("Checkout installation scope has an invalid size")
                            }
                        }
                        FORMAT_VERSION -> when (data.readUnsignedByte()) {
                            0 -> CheckoutProvisioningState.PENDING_ENROLLMENT.also {
                                if (bytes.size !in MIN_V2_PAYLOAD_BYTES..MAX_PAYLOAD_BYTES) {
                                    throw IOException("Checkout installation scope has an invalid size")
                                }
                            }
                            1 -> CheckoutProvisioningState.COMPLETED.also {
                                if (bytes.size !in MIN_V2_PAYLOAD_BYTES..MAX_PAYLOAD_BYTES) {
                                    throw IOException("Checkout installation scope has an invalid size")
                                }
                            }
                            else -> throw IOException("Checkout installation state is unsupported")
                        }
                        else -> throw IOException("Checkout installation scope format is unsupported")
                    }
                    val parts = List(5) {
                        val size = data.readUnsignedShort()
                        if (size !in 1..MAX_PART_BYTES) throw IOException("Checkout installation scope has an invalid field")
                        val raw = ByteArray(size).also(data::readFully)
                        val value = raw.toString(Charsets.UTF_8)
                        if (!value.toByteArray(Charsets.UTF_8).contentEquals(raw)) {
                            throw IOException("Checkout installation scope has invalid text")
                        }
                        value
                    }
                    if (data.available() != 0) throw IOException("Checkout installation scope has trailing data")
                    return CheckoutInstallationDescriptor(
                        CheckoutAuthorityScope(parts[0], parts[1], parts[2], parts[3], parts[4]),
                        provisioningState,
                    )
                }
            } catch (error: IOException) {
                throw error
            } catch (_: Exception) {
                throw IOException("Checkout installation scope is invalid")
            }
        }
    }
}
