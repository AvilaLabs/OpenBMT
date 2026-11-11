#!/usr/bin/env bash
# The Counter app's APK, built without Gradle: see apps/counter-android/README.md.
# The build and its test APK end up under ~/.cache/openbmt/target/counterapk/apk,
# never in the repository.
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/apps/counter-android/build.sh" "$@"
