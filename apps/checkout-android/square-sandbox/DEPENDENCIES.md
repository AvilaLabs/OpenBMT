# Square Sandbox dependency and contact inventory

**Scope:** the optional `apps/checkout-android/square-sandbox/` debug build.
It is separate from the no-Square native checkout and is not authorized for
production use or redistribution. This inventory records the artifacts and
metadata inspected on 2026-11-16. It is not a full third-party license review
or a network/telemetry audit.

## Direct vendor artifacts

| Coordinate | Purpose here | SHA-256 (AAR) |
|---|---|---|
| `com.squareup.sdk:mobile-payments-sdk:2.7.0` | In-person Square card-reader/payment SDK and Square-owned payment UI | `4844d0e0c7d3c52d1c663280a893b28b43e52e28e3c95ce4ca7cd2a7a523b602` |
| `com.squareup.sdk:mockreader-ui:2.7.0` | Sandbox-only simulated reader UI for approved/declined card exercises | `daaeee79a8493e0587b5830f84d829702696690a2bb073b730320ad81c80c06f` |

The hashes were checked against the AAR files downloaded into the coordinator's
temporary SDK cache. They are also checked by Gradle dependency verification
metadata in this subproject. The artifact POMs do not include a `<licenses>`
block. Neither AAR contains a `LICENSE` or `NOTICE` file. Square's quickstart
sample being Apache-2.0 licensed does not grant that license to these binary
SDKs. Obtain and review the vendor license terms and a complete license
inventory for all packaged transitive artifacts before any redistribution;
production packaging remains disabled in this module.

## Published transitive declarations

The MPS 2.7.0 POM declares runtime dependency
`com.squareup.sdk:mobile-payments-sdk-internals:[2.7.0]`. The corresponding
internal POM in the Gradle cache declares 162 compile dependencies at explicit
versions. They include AndroidX UI, lifecycle, persistence and security
components; Compose 1.12.1; Kotlin 2.3.21 and coroutines 1.10.2; Square Wire,
Workflow, OkHttp 5.3.2 and Retrofit 2.11.0; Google Play Integrity and Play
Services; and Datadog Android core/log/trace 2.25.0 plus OpenTracing. The
`mockreader-ui:2.7.0` POM points to `mockreader-ui-internals:2.7.0`, whose
published POM declares no further dependencies. Versions in these published
POMs are visible in the local artifact cache; their licenses and the final
resolved/packaged graph have not had a complete human review. The MPS AAR also
contains native `libregister.so` and `libbugsnag-root-detection.so` files.
Their runtime purpose and network behavior have not been independently
audited here.

These dependencies are added to the isolated debug build only. The default
checkout build does not resolve the Square artifacts. This module does not
add the Square SDK to the project's Rust `deps/` lists because those lists
describe the existing Rust programs; this temporary Android variant instead
has its own separate Gradle verification metadata. See the root dependency
policy and the [Square-specific source/build notes](README.md).

## Build tool pins

| Tool | Version/configuration |
|---|---|
| Gradle distribution | 9.3.1 binary distribution; SHA-256 `b266d5ff6b90eada6dc3b20cb090e3731302e553a27c5d3e4df1f0d76beaff06` |
| Android Gradle Plugin | 9.1.1; Android documents API 37.0 support in the [AGP 9.1.0 release notes](https://developer.android.com/build/releases/agp-9-1-0-release-notes), including patch 9.1.1 |
| JDK / SDK | JDK 21; compile SDK 37.0; target SDK 36; min SDK 29; Build Tools 37.0.0 |

Square's [Android MPS requirements](https://developer.squareup.com/docs/mobile-payments-sdk/android)
require AndroidX or Jetifier. This project enables both. The MPS 2.7.0
internal POM declares Compose 1.12.1. No credentials are build inputs. Sandbox
credentials are read at runtime from an external properties file imported to
app-private no-backup storage by the debug-only helper. The debug Activity
requests precise location, phone state, and Android 12+ Bluetooth permissions
before authorization; these grants do not by themselves establish which
device information Square collects or transmits.

## Network and data boundary

The Square integration is an explicit exception to the default checkout's
no-provider-network design. It uses Square's **Sandbox** environment only.
The app's own reconciliation client makes an HTTPS `GET` to
`connect.squareupsandbox.com/v2/payments`, with `location_id`, `total`,
`limit`, and an optional pagination cursor; it sends a runtime Sandbox bearer
token and `Square-Version` header. The Mobile Payments SDK also communicates
with Square to authorize the Sandbox seller/location, present its payment
flow, and process payments. Its complete destination list was not established
by inspecting the AAR/POM. See Square's [payment error definitions](https://developer.squareup.com/docs/mobile-payments-sdk/android/handling-errors),
[take-payment and cancellation behavior](https://developer.squareup.com/docs/mobile-payments-sdk/android/take-payments),
and [ListPayments API](https://developer.squareup.com/reference/square/payments-api/ListPayments).

The integration intentionally gives the SDK the Sandbox application ID,
Sandbox access token, seller location ID, amount/currency, and the durable
payment-attempt/reference UUID. The buyer enters or presents card data through
the Square-controlled SDK/reader flow. The SDK handles raw card numbers and
security codes; its callback returns a Payment object to the bridge, which
uses only the payment ID/status/amount/reference/source fields and does not
read or persist the card-detail fields. Square necessarily receives the
payment data needed to process that card-present transaction. The MPS dependency graph
includes Datadog log/trace libraries, but that alone does not establish
whether, when, or what the SDK sends. This project makes no claim that the SDK
has no telemetry. Exact runtime telemetry, destinations, retention, and the
device/location data observed by the SDK remain unverified pending a physical
device network/privacy audit.

The bridge interprets the SDK's literal `PaymentErrorCode.CANCELED` as
`CardResult.CANCELED`, because Square documents that callback code as a
payment canceled before completion. It interprets `CONSENT_NOT_PROVIDED` as
a preflight `FAILED` result: Square documents that starting before the
required seller consent is granted or denied fails before the payment prompt.
Both outcomes use synthetic local evidence IDs, not Square payment IDs. Other
SDK error codes are logged by enum name only in this debug source set;
timeout, authorization, network, location, clock, usage, obsolete-SDK, and
unknown errors stay unresolved and require reconciliation. No SDK error
message, debug payload, token, or card data is logged.

The debug Gradle build also accepts the build-time developer fault-injection
option `OPENBMT_SANDBOX_CALLBACK_DELAY_MS`, a decimal integer from 0 to 30000.
Its default is 0. A nonzero value delays only local delivery of an already
validated SDK `COMPLETED` callback, allowing a real Sandbox payment to test
crash/reconciliation recovery. It cannot manufacture or mark a payment paid.
The value is a debug-only `BuildConfig` field; the release variant is disabled.

The bridge does not intentionally send Bitcoin addresses, transaction IDs,
wallet descriptors, seed material, or wallet spending data to Square. Its
reference is the local payment-attempt UUID. A network trace and privacy review
are required before widening this experimental path or making broader claims
about its contacts. The module uses Android INTERNET permission and direct
Square Sandbox HTTPS; Bitcoin/Tor routing behavior does not cover this Square
connection.

For the application-wide scope statement, see [Network reference](../../../docs/handbook/network-reference.md#square-sandbox-exception)
and [Privacy](../../../docs/handbook/privacy.md#optional-square-sandbox-checkout).
