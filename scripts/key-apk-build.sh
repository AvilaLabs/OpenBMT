#!/usr/bin/env bash
# The Key app's APK, built without Gradle: see apps/key-android/README.md.
# The build and its test APK end up under ~/.cache/openbmt/target/keyapk/apk,
# never in the repository.
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/apps/key-android/build.sh" "$@"
