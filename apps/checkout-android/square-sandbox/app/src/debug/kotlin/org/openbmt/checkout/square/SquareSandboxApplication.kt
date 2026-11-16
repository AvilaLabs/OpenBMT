package org.openbmt.checkout.square

import android.app.Application
import com.squareup.sdk.mobilepayments.MobilePaymentsSdk
import java.io.File

/** A missing external credential file leaves the native checkout usable without Square. */
class SquareSandboxApplication : Application() {
    internal var credentials: SquareSandboxCredentials? = null
        private set

    override fun onCreate() {
        super.onCreate()
        credentials = runCatching {
            SquareSandboxCredentials.load(File(noBackupFilesDir, CREDENTIALS_FILE))
        }.getOrNull()
        credentials?.let { MobilePaymentsSdk.initialize(it.applicationId, this) }
    }

    companion object {
        const val CREDENTIALS_FILE = "square-sandbox.properties"
    }
}
