# Kotlin OBMn decoder check

`ProtocolDecoderTest.kt` is a plain JVM regression harness for the production
`decodeResponse` function and the request encoder in `CounterNode.kt`. It has
no test-framework or runtime dependencies beyond the Kotlin standard library
and JDK. It checks valid success and error frames, a screen with its words,
payment code and reports, and a pump result; it rejects undefined statuses,
tones, persons and tags, malformed UTF-8, a payment whose modules are not
`size * size` bits, a truncated error and trailing bytes; and it compares the
bytes of a `start` request and of a `judge` request with the bytes the Rust
encoder writes (the Rust tests hold the same literal).

Use JDK 21 and Kotlin/JVM 2.2.20. Set these variables to the local Kotlin
compiler runtime and standard-library jars:

- `JAVA_HOME`: the JDK root.
- `KOTLIN_COMPILER_CP`: classpath containing the Kotlin 2.2.20 embeddable
  compiler and its runtime dependencies.
- `KOTLIN_STDLIB_JAR`: the Kotlin 2.2.20 standard library jar.
- `KOTLIN_ANNOTATIONS_JAR`: JetBrains annotations jar required by the
  compiler's source classpath.

CI obtains the compiler ZIP from the [JetBrains Kotlin 2.2.20
release](https://github.com/JetBrains/kotlin/releases/tag/v2.2.20) and checks
its SHA-256 against the digest published with the release asset before
extracting it. The Ubuntu 24.04 runner supplies JDK 21 through
`JAVA_HOME_21_X64`. The same job runs the harness of the Key app's binding.

Then run:

```sh
crates/openbmt-counter-ffi/bindings/tests/run-jvm.sh
```

The script compiles `Native.kt`, `CounterNode.kt` and the harness into a
temporary directory with warnings treated as errors, then runs
`org.openbmt.counter.ProtocolDecoderTest`. It removes the temporary classes
when finished. This does not compile or load the Rust JNI library or test
Android storage or UI integration.
