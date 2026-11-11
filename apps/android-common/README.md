# Code shared by the Android apps

The Key app and the counter app are built from their own sources plus the
files here. Plain `android.widget` and the Kotlin standard library only: no
Gradle, no AndroidX. `src/model/` has no Android class, so the JVM tests run
it; `src/android/` needs the framework.

| File | What it is |
|---|---|
| `src/model/WordsModel.kt`, `SayCheck.kt` | The words of a screen as text, and the checks of `docs/SCREEN-RULES.md` as far as text can be checked |
| `src/model/Format.kt`, `DurableFile.kt`, `SealedFile.kt` | Text helpers, the durable write, the sealed file |
| `src/model/QrModel.kt`, `QrNative.kt` | The camera scan's pure parts and the binding of the QR reader |
| `src/model/UrNative.kt`, `UrCall.kt` | The binding of the UR call and its framing in Kotlin (UR-21), read strictly |
| `src/model/AnimatedQr.kt` | The numbers of the animated code, the frame timing and layout, the codes drawn once each |
| `src/model/UrReading.kt`, `UrWords.kt` | One reading of a sequence of parts, and its words |
| `src/android/QrScanner.kt`, `ScanFlow.kt` | The camera, and the scan page with the permission asked only when Scan is tapped |
| `src/android/AnimatedQrView.kt` | A plain `View` that shows a message as a still code or as parts that change |
| `src/android/Keystore.kt`, `Ui.kt` | Keystore sealing, and the screens' widgets |

## Animated codes

A message too long for one QR code is shown as the parts of a BC-UR sequence
(`crates/openbmt-ur`, `docs/UR-RULES.md`). The native side is one JNI
function, `UrNative.call`, of `crates/openbmt-qr-jni`: one framed request in
and one framed reply out, with the operations "draw a text", "cut a message
into parts", "open a decoder", "give it a part", "take its message",
"drop it" and "read a cosigner key out of a finished code" (`UrCall.keyText`:
the key as the text the key ceremony takes, or the refusal with its words, as
`KeyTextOutcome`; `docs/UR-RULES.md`, UR-23 to UR-31). At most four decoders are open at once and a handle is never used
twice. The Kotlin framing is `UrCall`; a reply that is not exactly what its
operation gives is a fault, never partly read.

- **`AnimatedQrView`** cycles the parts at 5 a second, drawn black on white
  with a quiet zone of 4 modules and whole pixels a module, as large as fits.
  Each part is upper case, for the alphanumeric mode, at level L, with at most
  230 bytes of the message in a part so that no frame is over QR version 12.
  A message of one part is a still code. It draws only while it is on a
  window. `AnimatedQrView.make` draws every part first and gives words
  instead of a view when one cannot be drawn.
- **`ScanFlow`** with a `UrScan` keeps scanning after a code that starts with
  `ur:`, feeds each part to one decoder (`UrReading`), and shows "Reading the
  animated code: N of about M parts" until the message is complete and of a
  type the step takes. A refusal that starts the reading again (UR-12) is shown
  in words; misreads and parts of another code are skipped, with a word after
  a run of them. The decoder is dropped when the scan finishes, is cancelled or
  the screen is cleared. A plain code still ends the scan at once.
- **Types (UR-20).** `ur:bytes` carries OpenBMT's own framed objects and
  `ur:crypto-psbt` a plain PSBT to or from another wallet.

The words of every new state are in `UrWords`, under `docs/SCREEN-RULES.md`;
`CommonModelTest` runs each through `SayCheck`.

## Tests

- `tests/run-jvm.sh` runs `CommonModelTest` on the desktop JVM.
- `tests/qr-jni-test.sh` builds `libopenbmt_qr_jni.so` for the host and runs
  `QrNativeHarness` (frames read through the same calls the camera scan
  makes) and `UrNativeHarness` (the UR call through `UrCall`: the parts of
  sequences of both types, the QR version of every frame, a lossy shuffled
  sequence read back, the decoder handles).

Nothing here has run on a device.
