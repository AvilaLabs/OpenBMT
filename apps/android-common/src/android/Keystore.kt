package org.openbmt.app

import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.ProviderException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.SecretKeyFactory
import javax.crypto.spec.GCMParameterSpec

/** Where the key that wraps the saved state lives. */
enum class KeyLevel { StrongBox, Tee, Software }

/**
 * AES-256-GCM under a key of the Android Keystore that never leaves it. The
 * key is made in the StrongBox when the phone has one and in the trusted
 * environment otherwise; [level] says what the system reports, and the app
 * shows [AppWords.keyInSoftware] when that is software. Every write gets the
 * fresh random 12-byte IV the Keystore makes (randomized encryption is
 * required of the key, so the Keystore refuses a caller-chosen one).
 */
class KeystoreSealer(private val alias: String = ALIAS) : Sealer {
    private val keystore: java.security.KeyStore =
        java.security.KeyStore.getInstance(PROVIDER).also { it.load(null) }

    val level: KeyLevel

    init {
        if (!keystore.containsAlias(alias)) {
            try {
                generate(strongBox = true)
            } catch (_: ProviderException) {
                // No StrongBox on this phone (StrongBoxUnavailableException is one of these).
                generate(strongBox = false)
            }
        }
        level = levelOf(key())
    }

    private fun generate(strongBox: Boolean) {
        val spec = KeyGenParameterSpec.Builder(
            alias,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setRandomizedEncryptionRequired(true)
            .setIsStrongBoxBacked(strongBox)
            .build()
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
        gen.init(spec)
        gen.generateKey()
    }

    private fun key(): SecretKey =
        keystore.getKey(alias, null) as? SecretKey ?: throw IOException("the key of the saved state is missing")

    @Suppress("DEPRECATION")
    private fun levelOf(key: SecretKey): KeyLevel {
        val factory = SecretKeyFactory.getInstance(key.algorithm, PROVIDER)
        val info = factory.getKeySpec(key, KeyInfo::class.java) as KeyInfo
        return if (Build.VERSION.SDK_INT >= 31) {
            when (info.securityLevel) {
                KeyProperties.SECURITY_LEVEL_STRONGBOX -> KeyLevel.StrongBox
                KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT, KeyProperties.SECURITY_LEVEL_UNKNOWN_SECURE ->
                    KeyLevel.Tee
                else -> KeyLevel.Software
            }
        } else if (info.isInsideSecureHardware) {
            KeyLevel.Tee
        } else {
            KeyLevel.Software
        }
    }

    override fun seal(plain: ByteArray, aad: ByteArray): Sealed {
        try {
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.ENCRYPT_MODE, key())
            cipher.updateAAD(aad)
            val ct = cipher.doFinal(plain)
            return Sealed(cipher.iv, ct)
        } catch (e: GeneralSecurityException) {
            throw IOException("the saved state could not be encrypted", e)
        }
    }

    override fun open(sealed: Sealed, aad: ByteArray): ByteArray {
        try {
            val cipher = Cipher.getInstance(TRANSFORM)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, sealed.iv))
            cipher.updateAAD(aad)
            return cipher.doFinal(sealed.ciphertext)
        } catch (e: GeneralSecurityException) {
            throw IOException("the saved state does not decrypt", e)
        }
    }

    companion object {
        const val PROVIDER = "AndroidKeyStore"
        const val ALIAS = "openbmt-key-state-v1"
        private const val TRANSFORM = "AES/GCM/NoPadding"
    }
}
