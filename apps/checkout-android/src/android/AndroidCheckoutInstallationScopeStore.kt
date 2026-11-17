package org.openbmt.checkout

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.IOException
import java.security.KeyStore
import javax.crypto.Mac
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/** Android no-backup descriptor store with a Keystore marker that survives descriptor deletion. */
class AndroidCheckoutInstallationScopeStore(context: Context) {
    private val store = CheckoutInstallationScopeStore(
        context.applicationContext.noBackupFilesDir,
        AndroidCheckoutInstallationMarker(),
    )

    fun load(expectedScope: CheckoutAuthorityScope? = null): CheckoutAuthorityScope = store.load(expectedScope)

    fun loadDescriptor(expectedScope: CheckoutAuthorityScope? = null): CheckoutInstallationDescriptor =
        store.loadDescriptor(expectedScope)

    fun createOnFreshInstall(
        evidence: CheckoutFreshInstallEvidence,
        createScope: () -> CheckoutAuthorityScope,
    ): CheckoutAuthorityScope = store.createOnFreshInstall(evidence, createScope)

    fun createDescriptorOnFreshInstall(
        evidence: CheckoutFreshInstallEvidence,
        createScope: () -> CheckoutAuthorityScope,
    ): CheckoutInstallationDescriptor = store.createDescriptorOnFreshInstall(evidence, createScope)

    fun markCompleted(
        expectedScope: CheckoutAuthorityScope,
        authority: CheckoutAuthority,
    ): CheckoutInstallationDescriptor = store.markCompleted(expectedScope, authority)

    fun validateAuthorityRecord(authority: CheckoutAuthority): CheckoutAuthorityLoginProfile? =
        store.validateAuthorityRecord(authority)

    override fun toString(): String = "AndroidCheckoutInstallationScopeStore(redacted)"
}

private class AndroidCheckoutInstallationMarker : CheckoutInstallationMarker {
    override fun isEstablished(): Boolean = try {
        keyStore().containsAlias(ALIAS)
    } catch (_: Exception) {
        throw IOException("Checkout installation marker is unavailable")
    }

    override fun isDescriptorCommitted(): Boolean = try {
        keyStore().containsAlias(COMMITTED_ALIAS)
    } catch (_: Exception) {
        throw IOException("Checkout installation marker is unavailable")
    }

    override fun canResumeInitialCreation(): Boolean = try {
        val keyStore = keyStore()
        keyStore.containsAlias(PENDING_ALIAS) && !keyStore.containsAlias(COMMITTED_ALIAS)
    } catch (_: Exception) {
        throw IOException("Checkout installation marker is unavailable")
    }

    override fun establish(): Boolean {
        val keyStore = try { keyStore() } catch (_: Exception) {
            throw IOException("Checkout installation marker is unavailable")
        }
        if (keyStore.containsAlias(COMMITTED_ALIAS)) return false
        if (keyStore.containsAlias(ALIAS) && !keyStore.containsAlias(PENDING_ALIAS)) return false
        if (!keyStore.containsAlias(PENDING_ALIAS)) generateMarkerKey(PENDING_ALIAS)
        if (!keyStore().containsAlias(ALIAS)) generateMarkerKey(ALIAS)
        return true
    }

    private fun generateMarkerKey(alias: String) {
        try {
            val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_HMAC_SHA256, PROVIDER)
            generator.init(KeyGenParameterSpec.Builder(
                alias,
                KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY,
            ).setKeySize(256).setDigests(KeyProperties.DIGEST_SHA256).build())
            generator.generateKey()
        } catch (_: Exception) {
            if (!keyStore().containsAlias(alias)) throw IOException("Checkout installation marker could not be established")
        }
    }

    override fun commitDescriptor() {
        val keyStore = try { keyStore() } catch (_: Exception) {
            throw IOException("Checkout installation marker is unavailable")
        }
        if (keyStore.containsAlias(COMMITTED_ALIAS)) return
        try {
            generateMarkerKey(COMMITTED_ALIAS)
        } catch (_: Exception) {
            if (!keyStore().containsAlias(COMMITTED_ALIAS)) {
                throw IOException("Checkout installation marker could not be committed")
            }
        }
    }

    override fun authenticate(bytes: ByteArray): ByteArray = mac().apply { init(secretKey()) }.doFinal(bytes)

    override fun verify(bytes: ByteArray, authenticator: ByteArray): Boolean {
        if (authenticator.size != 32) return false
        return java.security.MessageDigest.isEqual(authenticate(bytes), authenticator)
    }

    private fun secretKey(): SecretKey = try {
        keyStore().getKey(ALIAS, null) as? SecretKey ?: throw IOException("Checkout installation marker is unavailable")
    } catch (_: Exception) {
        throw IOException("Checkout installation marker is unavailable")
    }

    private fun mac(): Mac = try {
        Mac.getInstance("HmacSHA256", PROVIDER)
    } catch (_: Exception) {
        throw IOException("Checkout installation marker is unavailable")
    }

    private fun keyStore(): KeyStore = KeyStore.getInstance(PROVIDER).also { it.load(null) }

    private companion object {
        const val PROVIDER = "AndroidKeyStore"
        const val ALIAS = "openbmt-checkout-install-marker-v1"
        const val PENDING_ALIAS = "openbmt-checkout-scope-pending-v1"
        const val COMMITTED_ALIAS = "openbmt-checkout-scope-committed-v1"
    }
}
