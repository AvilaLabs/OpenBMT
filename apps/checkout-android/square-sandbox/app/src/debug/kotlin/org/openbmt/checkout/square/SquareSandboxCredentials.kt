package org.openbmt.checkout.square

import java.io.File
import java.util.Properties

/** Credentials exist only in the debug app's private no-backup storage, never in the APK. */
internal data class SquareSandboxCredentials(
    val applicationId: String,
    val accessToken: String,
    val locationId: String,
) {
    companion object {
        fun load(file: File): SquareSandboxCredentials? {
            if (!file.isFile) return null
            val values = Properties().apply { file.inputStream().use(::load) }
            val applicationId = values.getProperty("applicationId")?.trim().orEmpty()
            val accessToken = values.getProperty("accessToken")?.trim().orEmpty()
            val locationId = values.getProperty("locationId")?.trim().orEmpty()
            require(applicationId.startsWith("sandbox-sq0idb-")) {
                "Only a Square Sandbox application ID is accepted"
            }
            require(accessToken.isNotEmpty() && locationId.isNotEmpty()) {
                "Sandbox access token and location ID are required"
            }
            return SquareSandboxCredentials(applicationId, accessToken, locationId)
        }
    }
}
