package org.openbmt.checkout

import android.content.Context
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.Sealed
import org.openbmt.app.Sealer
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.security.MessageDigest

/** Production authority persistence: no-backup files, an installation-scope Keystore key and CAS. */
class AndroidCheckoutAuthorityStorage(
    context: Context,
    scope: CheckoutAuthorityScope,
    name: String = DEFAULT_FILE_NAME,
) : CheckoutAuthorityStorage {
    /** Reopens only from the previously authenticated installation descriptor. */
    constructor(context: Context) : this(
        context,
        AndroidCheckoutInstallationScopeStore(context).load(),
    )

    private val delegate = SealedCheckoutAuthorityStorage(
        context.applicationContext.noBackupFilesDir,
        scope,
        AndroidAuthorityRecordSealer(KeystoreSealer(keyAlias(scope))),
        name,
    )

    override fun read(): ByteArray? = delegate.read()

    override fun compareAndSwap(expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean =
        delegate.compareAndSwap(expectedBytes, nextBytes)

    override fun toString(): String = "AndroidCheckoutAuthorityStorage(redacted)"

    companion object {
        private const val KEY_ALIAS_PREFIX = "openbmt-checkout-authority-v1-"
        private const val DEFAULT_FILE_NAME = "checkout-authority.obis"

        /** A deterministic, non-secret scope tag keeps unrelated installation keys independent. */
        private fun keyAlias(scope: CheckoutAuthorityScope): String {
            val bytes = ByteArrayOutputStream()
            DataOutputStream(bytes).use { out ->
                listOf(scope.merchantId, scope.storeId, scope.locationId, scope.registerId, scope.deviceId).forEach {
                    val encoded = it.toByteArray(Charsets.UTF_8)
                    out.writeShort(encoded.size)
                    out.write(encoded)
                }
            }
            val digest = MessageDigest.getInstance("SHA-256").digest(bytes.toByteArray())
            val hex = buildString(digest.size * 2) {
                for (byte in digest) append("0123456789abcdef"[(byte.toInt() ushr 4) and 0x0f])
                    .append("0123456789abcdef"[byte.toInt() and 0x0f])
            }
            return KEY_ALIAS_PREFIX + hex.take(32)
        }
    }
}

private class AndroidAuthorityRecordSealer(private val delegate: Sealer) : CheckoutAuthorityRecordSealer {
    override fun seal(plain: ByteArray, aad: ByteArray): CheckoutAuthoritySealedValue =
        delegate.seal(plain, aad).let { CheckoutAuthoritySealedValue(it.iv, it.ciphertext) }

    override fun open(sealed: CheckoutAuthoritySealedValue, aad: ByteArray): ByteArray =
        delegate.open(Sealed(sealed.iv, sealed.ciphertext), aad)
}
