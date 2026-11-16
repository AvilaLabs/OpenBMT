package org.openbmt.checkout.square

import android.os.Handler
import android.os.Looper
import android.util.Log
import com.squareup.sdk.mobilepayments.MobilePaymentsSdk
import com.squareup.sdk.mobilepayments.core.Result
import com.squareup.sdk.mobilepayments.payment.AdditionalPaymentMethod
import com.squareup.sdk.mobilepayments.payment.CurrencyCode
import com.squareup.sdk.mobilepayments.payment.Money
import com.squareup.sdk.mobilepayments.payment.Payment
import com.squareup.sdk.mobilepayments.payment.PaymentErrorCode
import com.squareup.sdk.mobilepayments.payment.PaymentParameters
import com.squareup.sdk.mobilepayments.payment.PromptMode
import com.squareup.sdk.mobilepayments.payment.PromptParameters
import com.squareup.sdk.mobilepayments.payment.ProcessingMode
import org.openbmt.checkout.BuildConfig
import org.openbmt.checkout.CardCheckoutBridge
import org.openbmt.checkout.CardResult

/** Square Mobile Payments SDK adapter. It does not log card or payment details. */
internal class SquareMpsBridge(private val credentials: SquareSandboxCredentials) : CardCheckoutBridge {
    override fun begin(
        attemptId: String,
        referenceId: String,
        amountMinor: Long,
        currency: String,
        listener: CardCheckoutBridge.Listener,
    ) {
        require(attemptId.isNotBlank() && referenceId == attemptId)
        require(amountMinor > 0)
        val currencyCode = CurrencyCode.valueOf(currency)
        val paymentManager = MobilePaymentsSdk.paymentManager()
        val parameters = PaymentParameters.Builder(
            amount = Money(amountMinor, currencyCode),
            processingMode = ProcessingMode.ONLINE_ONLY,
            allowCardSurcharge = false,
            paymentAttemptId = attemptId,
        )
            .locationId(credentials.locationId)
            .referenceId(referenceId)
            .autocomplete(true)
            .build()
        val prompt = PromptParameters(
            mode = PromptMode.DEFAULT,
            additionalPaymentMethods = emptyList<AdditionalPaymentMethod.Type>(),
        )

        // The caller durably records attemptId/referenceId before this SDK call.
        paymentManager.startPaymentActivity(parameters, prompt) { result ->
            when (result) {
                is Result.Success -> {
                    val online = result.value as? Payment.OnlinePayment
                    if (online == null) {
                        listener.onUncertain()
                        return@startPaymentActivity
                    }
                    val cardResult = when (online.status) {
                        Payment.Status.APPROVED -> CardResult.APPROVED
                        Payment.Status.COMPLETED -> CardResult.COMPLETED
                        Payment.Status.CANCELED -> CardResult.CANCELED
                        Payment.Status.FAILED -> CardResult.FAILED
                        Payment.Status.UNKNOWN -> null
                    }
                    val reportedMoney = online.amountMoney
                    val callbackMatchesAttempt = online.referenceId == referenceId &&
                        online.locationId == credentials.locationId &&
                        online.sourceType == Payment.SourceType.CARD &&
                        reportedMoney.amount == amountMinor &&
                        reportedMoney.currencyCode == currencyCode
                    if (cardResult == null || !callbackMatchesAttempt || online.id.isBlank()) {
                        listener.onUncertain()
                    } else if (
                        cardResult == CardResult.COMPLETED &&
                        BuildConfig.SQUARE_CALLBACK_DELAY_MS > 0L
                    ) {
                        val delayMs = BuildConfig.SQUARE_CALLBACK_DELAY_MS
                        Log.w(TAG, "TEST ONLY: delaying validated COMPLETED callback by ${delayMs} ms")
                        Handler(Looper.getMainLooper()).postDelayed(
                            { listener.onEvidence(online.id, cardResult, reportedMoney.amount) },
                            delayMs,
                        )
                    } else {
                        listener.onEvidence(online.id, cardResult, reportedMoney.amount)
                    }
                }
                is Result.Failure -> {
                    // Enum only: never log error text/debug payloads, payment/card fields,
                    // credentials, or customer data. This source is debug-variant only.
                    Log.w(TAG, "Square payment callback errorCode=${result.errorCode.name}")
                    if (result.errorCode == PaymentErrorCode.CANCELED) {
                        // MPS documents this callback error as a payment canceled before
                        // completion. Keep it distinct from FAILED; all other callback errors
                        // may race a payment at Square and have no matching Payment object.
                        listener.onEvidence("square-canceled:$attemptId", CardResult.CANCELED, 0)
                    } else if (result.errorCode == PaymentErrorCode.CONSENT_NOT_PROVIDED) {
                        // Square documents that starting before the required seller consent
                        // is granted or denied fails with this code; no payment prompt starts.
                        listener.onEvidence(
                            "square-preflight:consent-not-provided:$attemptId",
                            CardResult.FAILED,
                            0,
                        )
                    } else {
                        listener.onUncertain()
                    }
                }
            }
        }
    }

    /** Read-only lookup only; it never resumes the payment prompt or issues a payment mutation. */
    override fun reconcile(
        attemptId: String,
        referenceId: String,
        amountMinor: Long,
        currency: String,
        listener: CardCheckoutBridge.Listener,
    ) {
        require(attemptId.isNotBlank() && referenceId == attemptId)
        // Query by the persisted reference ID. The SDK key is available separately for manual recovery.
        SquareSandboxReconciliation(credentials).query(referenceId, amountMinor, currency) { outcome ->
            if (outcome == null) listener.onUncertain()
            else listener.onEvidence(outcome.evidenceId, outcome.result, outcome.amountMinor)
        }
    }

    /**
     * The SDK retains its generated key locally for 24 hours. A caller may use it
     * as one input to reconciliation; a missing key/search result is not proof
     * that a charge cannot still arrive.
     */
    fun localIdempotencyKey(attemptId: String): String? =
        when (val result = MobilePaymentsSdk.paymentManager().getIdempotencyKey(attemptId)) {
            is Result.Success -> result.value
            is Result.Failure -> null
        }

    private companion object {
        const val TAG = "SquareSandbox"
    }
}
