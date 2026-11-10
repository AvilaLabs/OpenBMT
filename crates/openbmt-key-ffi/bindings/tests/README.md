# Kotlin OBM9 decoder check

`ProtocolDecoderTest.kt` is a plain JVM regression harness for the production
`decodeResponse` function in `KeyApp.kt`. It has no test-framework or runtime
dependencies beyond the Kotlin standard library and JDK. It checks valid
success and error frames, then rejects undefined statuses and word tags,
malformed UTF-8, a truncated error and an error with trailing bytes.

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
`JAVA_HOME_21_X64`.

Then run:

```sh
crates/openbmt-key-ffi/bindings/tests/run-jvm.sh
```

The script compiles `Native.kt`, `KeyApp.kt` and the harness into a temporary
directory with warnings treated as errors, then runs
`org.openbmt.key.ProtocolDecoderTest`. It removes the temporary classes when
finished. This does not compile or load the Rust JNI library or test Android
storage, keystore behavior or UI integration.
