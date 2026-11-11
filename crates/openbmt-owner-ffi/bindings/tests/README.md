# Kotlin OBMl decoder check

`ProtocolDecoderTest.kt` is a plain JVM regression harness for the production
`decodeResponse` function and the request encoder in `OwnerNode.kt`. It has
no test-framework or runtime dependencies beyond the Kotlin standard library
and JDK. It checks valid success and error frames, a status, a taken
response, the queue, the link state and the old-addresses line, each with
its words; it rejects undefined statuses, tones, persons and tags,
malformed UTF-8, a truncated error and trailing bytes; and it compares the
bytes of a `start` request and of a `pump` request with the bytes the Rust
encoder writes (the Rust tests hold the same literal).

Use JDK 21 and Kotlin/JVM 2.2.20. Set these variables to the local Kotlin
compiler runtime and standard-library jars:

- `JAVA_HOME`: the JDK root.
- `KOTLIN_COMPILER_CP`: classpath containing the Kotlin 2.2.20 embeddable
  compiler and its runtime dependencies.
- `KOTLIN_STDLIB_JAR`: the Kotlin 2.2.20 standard library jar.
- `KOTLIN_ANNOTATIONS_JAR`: JetBrains annotations jar required by the
  compiler's source classpath.
- `OWNER_FFI_LIB_DIR` (optional): the directory that holds
  `libopenbmt_owner_ffi.so`. When it is not set the script runs
  `cargo build --locked -p openbmt-owner-ffi` and uses
  `$CARGO_TARGET_DIR/debug` (or `target/debug`).

CI sets these up in the Kotlin job, which also runs the harnesses of the
Key app's and the counter's bindings. Then run:

```sh
crates/openbmt-owner-ffi/bindings/tests/run-jvm.sh
```

The script compiles `Native.kt`, `OwnerNode.kt` and the harness into a
temporary directory with warnings treated as errors, then runs
`org.openbmt.owner.ProtocolDecoderTest` with the library on
`java.library.path`. It removes the temporary classes when finished. This
does not test Android storage or UI integration.
