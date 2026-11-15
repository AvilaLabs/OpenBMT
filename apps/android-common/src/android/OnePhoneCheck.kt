package org.openbmt.app

import android.content.Context
import android.content.pm.PackageManager

/**
 * The same-publisher check of one-phone mode (CR-277, OA-124): the system is asked whether another package is signed
 * with the certificate this app is signed with (`hasSigningCertificate`, SHA-256). The judgment is [OnePhone]'s.
 */
object OnePhoneCheck {
    /** The SHA-256 of this app's one signing certificate; null when it cannot be read or the app has several signers. */
    fun ownDigest(context: Context): ByteArray? {
        val info = try {
            context.packageManager.getPackageInfo(context.packageName, PackageManager.GET_SIGNING_CERTIFICATES)
        } catch (_: PackageManager.NameNotFoundException) {
            return null
        }
        val signing = info.signingInfo ?: return null
        if (signing.hasMultipleSigners()) return null
        return OnePhone.ownDigest(signing.apkContentsSigners.map { it.toByteArray() })
    }

    /** Whether [other] is signed with this app's certificate; false when it is not installed or the digest cannot be made. */
    fun sameSigner(context: Context, other: String): Boolean {
        val digest = ownDigest(context) ?: return false
        return try {
            context.packageManager.hasSigningCertificate(other, digest, PackageManager.CERT_INPUT_SHA256)
        } catch (_: RuntimeException) {
            false
        }
    }

    /** Whether [other] is installed and visible to this app. */
    fun installed(context: Context, other: String): Boolean = try {
        context.packageManager.getPackageInfo(other, 0)
        true
    } catch (_: PackageManager.NameNotFoundException) {
        false
    }

    /** What this app finds of [other]: missing, signed by another publisher, or the same one. */
    fun peer(context: Context, other: String): OnePhone.Peer = OnePhone.peer(installed(context, other), sameSigner(context, other))
}
