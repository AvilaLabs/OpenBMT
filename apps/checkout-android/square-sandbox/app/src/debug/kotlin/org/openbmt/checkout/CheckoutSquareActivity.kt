package org.openbmt.checkout

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import com.squareup.sdk.mobilepayments.core.CallbackReference
import com.squareup.sdk.mobilepayments.core.Result
import com.squareup.sdk.mobilepayments.MobilePaymentsSdk
import com.squareup.sdk.mobilepayments.mockreader.ui.MockReaderUI
import org.openbmt.checkout.square.SquareMpsBridge
import org.openbmt.checkout.square.SquareSandboxApplication

/** Optional debug-only host. The base CheckoutActivity remains provider-free. */
class CheckoutSquareActivity : CheckoutActivity() {
    private var authorizationCallback: CallbackReference? = null
    private var authorizationStateCallback: CallbackReference? = null
    private var authorized = false
    private var authorizationPending = false

    override fun onResume() {
        super.onResume()
        if ((application as SquareSandboxApplication).credentials == null) return
        val manager = MobilePaymentsSdk.authorizationManager()
        if (authorizationStateCallback == null) {
            authorizationStateCallback = manager.setAuthorizationStateChangedCallback { state ->
                authorized = state.isAuthorized
                authorizationPending = state.isAuthorizationInProgress
                if (authorized && MobilePaymentsSdk.isSandboxEnvironment()) MockReaderUI.show()
            }
        }
        authorizeIfReady()
    }

    private fun authorizeIfReady() {
        val app = application as SquareSandboxApplication
        val credentials = app.credentials ?: return
        val missing = requiredRuntimePermissions().filter {
            checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
        }
        if (missing.isNotEmpty()) {
            requestPermissions(missing.toTypedArray(), PERMISSIONS_REQUEST)
            return
        }
        val authorizationManager = MobilePaymentsSdk.authorizationManager()
        val sdkState = authorizationManager.authorizationState
        if (sdkState.isAuthorized) {
            authorized = true
            authorizationPending = false
            if (MobilePaymentsSdk.isSandboxEnvironment()) MockReaderUI.show()
            return
        }
        authorized = false
        if (sdkState.isAuthorizationInProgress) {
            authorizationPending = true
            return
        }
        authorizationPending = false
        if (!authorized && !authorizationPending) {
            authorizationPending = true
            authorizationCallback = authorizationManager
                .authorize(credentials.accessToken, credentials.locationId) { result ->
                    authorizationPending = false
                    when (result) {
                        is Result.Success -> {
                            authorized = true
                            if (MobilePaymentsSdk.isSandboxEnvironment()) MockReaderUI.show()
                        }
                        is Result.Failure -> authorized = false
                    }
                }
        }
    }

    override fun onPause() {
        authorizationCallback?.clear()
        authorizationCallback = null
        authorizationStateCallback?.clear()
        authorizationStateCallback = null
        authorizationPending = false
        super.onPause()
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == PERMISSIONS_REQUEST && grantResults.all { it == PackageManager.PERMISSION_GRANTED }) {
            authorizeIfReady()
        }
    }

    override fun createCardCheckoutBridge(): CardCheckoutBridge? {
        if (!authorized || !MobilePaymentsSdk.isSandboxEnvironment()) return null
        val credentials = (application as SquareSandboxApplication).credentials ?: return null
        return SquareMpsBridge(credentials)
    }

    private fun requiredRuntimePermissions(): List<String> = buildList {
        add(Manifest.permission.ACCESS_FINE_LOCATION)
        add(Manifest.permission.READ_PHONE_STATE)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            add(Manifest.permission.BLUETOOTH_CONNECT)
            add(Manifest.permission.BLUETOOTH_SCAN)
        }
    }

    private companion object {
        const val PERMISSIONS_REQUEST = 4742
    }
}
