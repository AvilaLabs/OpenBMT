package org.openbmt.owner.app

import android.app.KeyguardManager
import android.content.pm.PackageManager
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import android.security.keystore.UserNotAuthenticatedException
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.UnrecoverableKeyException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

// The starter wallet's Keystore key and the proof that the owner is there (SS-20, SS-21). The Rust core seals the seed
// under a random data key and gives the app that key once; this key wraps it. The data key is a byte array, never a
// String, and is overwritten as soon as the call that needs it has answered. Only platform APIs are used: the
// Keystore, and the platform's BiometricPrompt of `android.hardware.biometrics`.

/** The Keystore has no usable key of the starter: it was never made, or the phone dropped it (SS-23). */
class StarterKeyGone(cause: Throwable? = null) : Exception("the starter's Keystore key is gone", cause)

/**
 * AES-256-GCM under a key of the Android Keystore made for the starter alone: in StrongBox when the phone has it,
 * never exportable, usable only with the phone unlocked and only after the owner proves they are there (on Android 11
 * and later for every use, by a strong biometric or the phone's own PIN; on Android 10 within 10 seconds of the proof).
 */
class StarterKeystore {
    private val keystore: KeyStore = KeyStore.getInstance(PROVIDER).also { it.load(null) }

    fun exists(): Boolean = try {
        keystore.containsAlias(ALIAS)
    } catch (_: java.security.KeyStoreException) {
        false
    }

    /** SS-20, SS-21: makes the key (and replaces one that is there). [strongBox]: the phone has `FEATURE_STRONGBOX_KEYSTORE`. */
    fun make(strongBox: Boolean) {
        delete()
        try {
            generate(strongBox)
        } catch (_: StrongBoxUnavailableException) {
            // The feature is listed and the chip refused: the same key in the trusted environment.
            generate(false)
        }
    }

    private fun generate(strongBox: Boolean) {
        val b = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setRandomizedEncryptionRequired(true)
            .setUserAuthenticationRequired(true)
            .setUnlockedDeviceRequired(true)
        if (Build.VERSION.SDK_INT >= 30) {
            b.setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG or KeyProperties.AUTH_DEVICE_CREDENTIAL)
        } else {
            @Suppress("DEPRECATION")
            b.setUserAuthenticationValidityDurationSeconds(VALIDITY_SECS)
        }
        if (strongBox) b.setIsStrongBoxBacked(true)
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
        gen.init(b.build())
        gen.generateKey()
    }

    /** SS-43: the key is deleted. */
    fun delete() {
        try {
            if (keystore.containsAlias(ALIAS)) keystore.deleteEntry(ALIAS)
        } catch (_: java.security.KeyStoreException) {
        }
    }

    private fun key(): SecretKey = try {
        keystore.getKey(ALIAS, null) as? SecretKey ?: throw StarterKeyGone()
    } catch (e: UnrecoverableKeyException) {
        throw StarterKeyGone(e)
    } catch (e: java.security.KeyStoreException) {
        throw StarterKeyGone(e)
    }

    /** A cipher to wrap a data key. Throws [StarterKeyGone] when the key is gone. Before Android 11 it works only after the proof. */
    fun encryptCipher(): Cipher = cipher { it.init(Cipher.ENCRYPT_MODE, key()) }

    /** A cipher to unwrap a data key wrapped with [iv]. Throws [StarterKeyGone] when the key is gone. */
    fun decryptCipher(iv: ByteArray): Cipher = cipher { it.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, iv)) }

    private fun cipher(init: (Cipher) -> Unit): Cipher {
        val c = Cipher.getInstance(TRANSFORM)
        try {
            init(c)
        } catch (e: KeyPermanentlyInvalidatedException) {
            throw StarterKeyGone(e)
        }
        return c
    }

    /** Whether the key is there and has not been dropped by the phone (a key that wants the proof is still usable). */
    fun usable(): Boolean = try {
        decryptCipher(ByteArray(IV_BYTES))
        true
    } catch (_: StarterKeyGone) {
        false
    } catch (_: UserNotAuthenticatedException) {
        true
    } catch (_: GeneralSecurityException) {
        exists()
    }

    companion object {
        const val PROVIDER = "AndroidKeyStore"
        const val ALIAS = StarterStore.KEY_ALIAS
        private const val TRANSFORM = "AES/GCM/NoPadding"
        private const val IV_BYTES = 12
        private const val VALIDITY_SECS = 10
        private val AAD = "OpenBMT/starter/data-key/v1".toByteArray(Charsets.UTF_8)

        /** Wraps [dataKey] with an authenticated [cipher] and overwrites [dataKey] with zeros. */
        fun wrap(cipher: Cipher, dataKey: ByteArray): ByteArray {
            try {
                cipher.updateAAD(AAD)
                return StarterWrap.pack(cipher.iv, cipher.doFinal(dataKey))
            } finally {
                dataKey.fill(0)
            }
        }

        /** Opens a wrapped data key with an authenticated [cipher]. The caller overwrites what comes back as soon as it is used. */
        fun unwrap(cipher: Cipher, ciphertext: ByteArray): ByteArray {
            cipher.updateAAD(AAD)
            return cipher.doFinal(ciphertext)
        }
    }
}

// ---- the owner is there (SS-21) ----------------------------------------------------------------------------------

/**
 * Asks the owner to prove they are there. From Android 11 the platform's prompt takes a strong biometric or the PIN
 * (only the PIN when [credentialOnly]), and the proof is tied to [cipher] so that the key works once, for that use.
 * On Android 10 the key works for 10 seconds after the proof: the prompt takes the PIN or a fingerprint, and
 * [credentialOnly] is the phone's own PIN screen. [onOk] gets the cipher the proof opened (null on Android 10, where
 * the caller makes it now); [onFail] is anything else, the owner's cancelling included. The prompt's texts are the
 * placeholders of [StarterWords].
 */
internal fun Controller.starterAuthenticate(
    cipher: Cipher?,
    credentialOnly: Boolean,
    description: String,
    onOk: (Cipher?) -> Unit,
    onFail: () -> Unit,
) {
    if (Build.VERSION.SDK_INT < 30 && credentialOnly) {
        val keyguard = act.getSystemService(KeyguardManager::class.java)
        @Suppress("DEPRECATION")
        val intent = keyguard?.createConfirmDeviceCredentialIntent(StarterWords.authTitle, description)
        if (intent == null) return onFail()
        return startCredential(intent) { ok -> if (ok) onOk(null) else onFail() }
    }
    val builder = BiometricPrompt.Builder(act)
        .setTitle(StarterWords.authTitle)
        .setSubtitle(StarterWords.authSubtitle)
        .setDescription(description)
    if (Build.VERSION.SDK_INT >= 30) {
        builder.setAllowedAuthenticators(
            if (credentialOnly) BiometricManager.Authenticators.DEVICE_CREDENTIAL
            else BiometricManager.Authenticators.BIOMETRIC_STRONG or BiometricManager.Authenticators.DEVICE_CREDENTIAL,
        )
    } else {
        @Suppress("DEPRECATION")
        builder.setDeviceCredentialAllowed(true)
    }
    val callback = object : BiometricPrompt.AuthenticationCallback() {
        override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
            onOk(result.cryptoObject?.cipher)
        }

        override fun onAuthenticationError(errorCode: Int, errString: CharSequence?) {
            onFail()
        }
    }
    val prompt = builder.build()
    val signal = CancellationSignal()
    if (cipher != null && Build.VERSION.SDK_INT >= 30) {
        prompt.authenticate(BiometricPrompt.CryptoObject(cipher), signal, act.mainExecutor, callback)
    } else {
        prompt.authenticate(signal, act.mainExecutor, callback)
    }
}

/** Whether the phone has the StrongBox (SS-20). */
internal fun Controller.hasStrongBox(): Boolean =
    act.packageManager.hasSystemFeature(PackageManager.FEATURE_STRONGBOX_KEYSTORE)

/**
 * SS-20, SS-21: wraps the core's [dataKey] with the Keystore key after the owner proves they are there, and keeps the
 * wrapped key. [dataKey] is overwritten whatever happens. [done] says whether it was kept.
 */
internal fun Controller.starterWrapKey(dataKey: ByteArray, description: String, done: (Boolean) -> Unit) {
    val keystore = starter.keystore
    try {
        keystore.make(hasStrongBox())
        val pre = if (Build.VERSION.SDK_INT >= 30) keystore.encryptCipher() else null
        starterAuthenticate(pre, false, description, { c ->
            try {
                val cipher = c ?: keystore.encryptCipher()
                starter.store.saveWrapped(StarterKeystore.wrap(cipher, dataKey))
                done(true)
            } catch (_: GeneralSecurityException) {
                done(false)
            } catch (_: StarterKeyGone) {
                done(false)
            } catch (_: IOException) {
                done(false)
            } finally {
                dataKey.fill(0)
            }
        }, {
            dataKey.fill(0)
            done(false)
        })
    } catch (_: GeneralSecurityException) {
        dataKey.fill(0)
        done(false)
    } catch (_: StarterKeyGone) {
        dataKey.fill(0)
        done(false)
    }
}

/**
 * SS-20, SS-21, SS-42: opens the wrapped data key after the owner proves they are there ([credentialOnly]: with the
 * phone's PIN alone, the final move), hands it to [use] and overwrites it at once. [gone] is the cue for the restore
 * (SS-23): the Keystore key is missing or was dropped. [failed] is the owner not proving it, or any other failure.
 */
internal fun Controller.starterWithDataKey(
    credentialOnly: Boolean,
    description: String,
    use: (ByteArray) -> Unit,
    gone: () -> Unit,
    failed: () -> Unit,
) {
    val keystore = starter.keystore
    val parts = try {
        StarterWrap.unpack(starter.store.loadWrapped() ?: return gone())
    } catch (_: IOException) {
        return gone()
    }
    val pre = try {
        if (Build.VERSION.SDK_INT >= 30) keystore.decryptCipher(parts.iv) else null
    } catch (_: StarterKeyGone) {
        return gone()
    } catch (_: GeneralSecurityException) {
        return gone()
    }
    starterAuthenticate(pre, credentialOnly, description, { c ->
        val key: ByteArray? = try {
            StarterKeystore.unwrap(c ?: keystore.decryptCipher(parts.iv), parts.ciphertext)
        } catch (_: StarterKeyGone) {
            null.also { gone() }
        } catch (_: GeneralSecurityException) {
            null.also { failed() }
        }
        if (key != null) {
            try {
                use(key)
            } finally {
                key.fill(0)
            }
        }
    }, failed)
}
