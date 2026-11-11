#!/usr/bin/env bash
# The Owner app's APK, built without Gradle: see apps/owner-android/README.md.
# The build and its test APK end up under ~/.cache/openbmt/target/ownerapk/apk,
# never in the repository.
exec "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/apps/owner-android/build.sh" "$@"
