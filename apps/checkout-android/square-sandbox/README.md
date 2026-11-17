# Optional Square Mobile Payments SDK (Sandbox only)

This is a separate Gradle build for the experimental Android checkout. The
plain-widget app and its Bitcoin-only path do not depend on Square, AndroidX,
or Gradle. Only this project resolves Square's SDK and its AndroidX
transitives. It uses Square's **Mobile Payments SDK** for in-person readers;
it does not use the In-App Payments card-entry/nonce SDK.

## Pinned dependency

Artifact hashes, actual POM dependency declarations, tool pins, licensing
status, and the Sandbox network/data boundary are recorded in
[DEPENDENCIES.md](DEPENDENCIES.md).

- `com.squareup.sdk:mobile-payments-sdk:2.7.0` — Square's Android in-person
  payments client, including reader and payment flows.
- `com.squareup.sdk:mockreader-ui:2.7.0` — Square's virtual reader UI for
  Sandbox-only card acceptance/decline simulation. Do not package it in a
  production variant.
- Repository: `https://sdk.squareup.com/public/android/` (Square's Maven
  repository). Square's Android sample app is Apache License 2.0; that does
  not establish the license of the separately published SDK binaries. Preserve
  any vendor license/notice material that Square provides and inspect each
  artifact's POM and full resolved dependency graph before redistribution. The
  downloaded AARs did not contain license/notice files. The 2.7.0 SDK requires
  Jetpack Compose 1.12 and includes AndroidX transitive libraries. Exact
  resolved versions and licenses must be captured in the generated Gradle
  lock/license inventory before considering any release. These dependencies
  are confined to this optional Gradle project and absent from the native
  plain-widget APK.

The current Square Android guide (checked 2026-11-16) lists MPS 2.7.0, min API
28, compile SDK 37, AndroidX or Jetifier, AGP 9.1.0+, and Gradle 9.3.1+.
This project pins AGP 9.1.1 because its release notes document API 37.0 support.
Square requires current SDK versions under its SDK update policy. This project
is an experimental Sandbox integration and does not change the existing app's
build toolchain.

The Gradle app compiles the checkout Activity/model, shared Android helpers,
and counter FFI Kotlin bindings from this repository. Set `CHECKOUTAPK_OUT` to
the output directory of the feature-gated checkout build (the directory with
`stage/lib/arm64-v8a/` and `stage/lib/x86_64/`). It checks for the
`checkout-regtest` FFI, QR JNI, and Tor libraries there; it does not accept the
regular Counter APK libraries or build substitute native libraries. The
integration stays in the debug build variant.

## Sandbox credentials

The external properties file selected by `SQUARE_SANDBOX_CREDENTIALS` must
contain `applicationId`, `accessToken`, and `locationId` from the Square
Developer Dashboard's **Sandbox** view. Keep that file outside the repository
and out of version control. Gradle does not read the file and no credential is
compiled into the APK. Use `scripts/push-sandbox-credentials.sh` to copy it
into the debug app's private `noBackupFilesDir`; the app accepts only a
Sandbox application ID. The access token is read at runtime from that private
file for SDK authorization. Never use a production credential, distribute
the test APK, or reuse this credential flow for a production build.

### Separate debug trials

The default debug package is `org.openbmt.checkout.square.sandbox.debug`; it
retains its existing app-private ledger. To isolate a new developer test,
set `OPENBMT_SANDBOX_TRIAL` to a lowercase identifier matching
`[a-z][a-z0-9_]{0,15}` both when building and when importing credentials. For
example, use `OPENBMT_SANDBOX_TRIAL=consent2` to create package
`org.openbmt.checkout.square.sandbox.debug.consent2`, then run the credential
helper with the same variable set. The helper derives this package name and
rejects invalid identifiers; it does not accept an arbitrary package name.
This only separates app data for developer testing. It does not recover,
clear, or reclassify an `UNKNOWN` attempt in the default app; that ledger
remains in its original installation and still requires reconciliation.

Example: set `OPENBMT_SANDBOX_TRIAL=consent2` for the normal Gradle debug
build, then import the external Sandbox file with
`OPENBMT_SANDBOX_TRIAL=consent2 SQUARE_SANDBOX_CREDENTIALS=/path/to/sandbox.properties scripts/push-sandbox-credentials.sh`.

Use one property per line: `applicationId=...`, `accessToken=...`, and
`locationId=...`. After installing the APK, import it with
`SQUARE_SANDBOX_CREDENTIALS=/absolute/path/to/file scripts/push-sandbox-credentials.sh`.

MPS permissions are purpose-specific: precise location is required while in
use; Bluetooth permissions are needed for contactless/chip readers, and
microphone permission for magstripe. This Sandbox harness uses contactless/chip
mock readers, declares no microphone permission, and requests precise location,
phone-state, and (on Android 12+) Bluetooth permissions at runtime before
authorization. It declares the legacy Bluetooth permissions on older Android
versions. No production permissions or Square production activation are
implied by the optional integration.

## Payment contract

The host checkout creates and durably records the OpenBMT attempt and reference
IDs before invoking the bridge. The stable OpenBMT attempt ID is passed as
Square `paymentAttemptId`; the OpenBMT reference ID is passed as Square
`referenceId`. MPS creates/stores its own idempotency key for that attempt.
Card data stays inside Square's SDK; the bridge never receives PAN/CVV and
must not log card details, access tokens, or SDK error debug payloads.

The bridge uses `ProcessingMode.ONLINE_ONLY` and `autocomplete=true`. The
persisted OpenBMT UUID is used as both Square `paymentAttemptId` and
`referenceId`, so recovery can locate exactly the same attempt after restart.
SDK success means Square's in-person payment flow succeeded; the host still
maps only evidence it can validate to the ledger. An `APPROVED` status remains
pending and is never treated as captured/paid. A `COMPLETED` observation is
the only Square status that can settle the card attempt. Canceled/declined
flows are terminal only when the SDK callback says so. A missing callback,
app interruption, or unusable result leaves the attempt `UNKNOWN`; do not
show another card prompt or switch tender until reconciliation resolves it.

Square keeps the SDK-generated idempotency key locally for 24 hours and
`SquareMpsBridge.localIdempotencyKey(paymentAttemptId)` exposes that retained
value for recovery checks. The reconciliation button uses a separate,
read-only Square Sandbox `ListPayments` query by location and amount, then
requires exactly one exact `referenceId` match before returning evidence.
Square says these results are eventually consistent. An empty search result is
not proof that a payment cannot still arrive; keep the sale locked and
escalate to a deliberate merchant resolution step rather than retrying the
card. This module does not auto-cancel, refund, or create a second payment.

### Callback-loss recovery fault injection

For a real Sandbox recovery drill, set `OPENBMT_SANDBOX_CALLBACK_DELAY_MS` at
Gradle build time to a decimal integer from `0` through `30000`; unset or `0`
is the normal behavior. For example, `OPENBMT_SANDBOX_CALLBACK_DELAY_MS=10000`
delays only delivery of an already validated SDK `COMPLETED` observation to
the local ledger listener. Square processes the actual payment normally; this
does not fabricate SDK success, an evidence record, or a payment ID. Force-stop
the app before the local delay expires, relaunch it, and use the read-only
`ListPayments` reconciliation path. The setting is generated into debug
`BuildConfig` only; it does not alter other SDK statuses, errors, or
reconciliation. Use a separate `OPENBMT_SANDBOX_TRIAL` package to preserve the
default app's existing ledger. This fault injection is developer test support,
not a production feature.

## Manual Sandbox path

1. Configure the external properties file with Sandbox values only.
2. Build the debug variant with Gradle 9.3.1+ and the feature-verified native
   libraries selected through `CHECKOUTAPK_OUT`.
3. Install the debug APK and run `scripts/push-sandbox-credentials.sh` with the
   external file path to import credentials into the app-private directory.
4. Launch the debug harness and authorize against the Sandbox seller/location.
5. Open Square's floating Mock Reader UI and add a contactless/chip mock reader.
6. Start one card attempt from the checkout UI. Use Mock Reader UI's approved
   test-card path for success, then a fresh sale and its decline path to
   exercise a decline. Confirm each payment in the Sandbox Dashboard.

## Integrated checkout milestone on 16 November 2026

This milestone was recorded on 16 November in America/New_York (17 November
UTC). The Square Sandbox path was exercised from the native OpenBMT cart using
Mobile Payments SDK 2.7.0 and its virtual reader. One latte ($5.50) and one bagel
($4.50) produced a single $10.00 sale in the native ledger. The completed
virtual-card payment was independently confirmed by Square's read-only
Payments API; the Square payment ID and attempt reference matched the native
ledger.

The app was then force-stopped after payment. On relaunch, the native ledger
bytes were identical, and Sales showed the same itemized sale with a paid card
receipt. This verifies the completed-payment recovery case for this Sandbox
flow. It does not demonstrate recovery from termination during a pending SDK
request, when status is still unknown.

The build used the shared Regtest-only native libraries. Both Android ABIs,
the APK and SDK module builds, and native method-signature alignment passed.
The virtual reader is not physical hardware. This remains a Sandbox integration
proof, not production payment processing, a Square POS replacement, or evidence
of catalog, inventory, peripheral, iPad, onboarding, or multi-merchant support.

### Earlier exploratory Sandbox runs (16 November 2026)


The coordinator built this module with MPS 2.7.0, AGP 9.1.1 and Gradle 9.3.1,
then ran it on the API 35 Android emulator. The private Square application
used the personal testing audience and Sandbox credentials imported at runtime;
no production seller activation, bank connection, or Marketplace publication
was performed.

An $8.00 amount entered once in OpenBMT reached the SDK's payment prompt and
official contactless mock reader. The callback produced a native paid receipt;
Square's read-only Payments API independently returned `COMPLETED`, USD 800
minor units, and the same payment ID and attempt reference. The receipt
survived a forced process restart.

A second $5.00 sale was canceled before presenting a card. The SDK's definitive
`CANCELED` result recorded zero received and allowed a new attempt for that
sale's remaining balance. That attempt completed without re-entering the
amount; the ledger retained both attempts and Square confirmed the matching
USD 500 payment. The result receipt stayed visible after the counter restarted.

For an interruption test, an isolated build delayed local delivery of a
validated completion callback by ten seconds. Square completed a $3.00 virtual
card payment, then the coordinator force-stopped the app before local delivery.
After restart, the native attempt was `UNKNOWN` with zero recorded received.
Read-only reconciliation recovered `COMPLETED` using the original exact payment
ID and attempt reference. Square's location payment count stayed at three;
recovery created no additional payment. The deliberate callback delay was
removed from the normal test build afterward.

An earlier unknown $8.00 trial remained locked when read-only reconciliation
could not find its exact reference, despite a separate completed payment of
the same amount. That record was retained in its original test installation.
An idle test prompt also returned `TIMEOUT` without a payment record; its
attempt remains unknown. An empty lookup is not a safe retry authorization.
Recovery of these absent-record cases needs a validated explicit cancellation
or owner-resolution design before production use.
Square's [cancel-by-idempotency-key API](https://developer.squareup.com/reference/square/payments-api/cancel-payment-by-idempotency-key)
can succeed without taking action when no payment is found. Its empty success
response does not establish that a late request has been prevented; this
harness does not use it to unlock an unknown attempt.

These are SDK/backend integration tests using virtual cards, not physical
reader, iPad, merchant onboarding, catalog/inventory synchronization, or public
multi-merchant distribution tests. Actual refund execution is not implemented;
the shared checkout records refund requests only. The source's release variant
remains disabled, and the SDK's licensing, telemetry, and wallet-key isolation
gates in the [dependency inventory](DEPENDENCIES.md) remain open.

## References checked

- [Square Mobile Payments SDK for Android](https://developer.squareup.com/docs/mobile-payments-sdk/android)
- [Square Android Take a Payment guide](https://developer.squareup.com/docs/mobile-payments-sdk/android/take-payments)
- [Square List Payments API reference](https://developer.squareup.com/reference/square/payments-api/ListPayments)
- [Square Android quickstart sample](https://github.com/square/mobile-payments-sdk-android)
- [Square Mobile SDK release notes](https://developer.squareup.com/docs/changelog/mobile)
