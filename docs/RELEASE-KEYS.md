# Release keys

A release of OpenBMT is a folder of files, a list of their SHA-256 hashes
(`SHA256SUMS`), and a signature of that list (`SHA256SUMS.asc`) made with the
GPG key below. Each app is also signed with an Android key of its own, and
Android installs an update only when it is signed with the same key as the
app already on the phone, so an update from anyone else is refused.

To check a release you downloaded:

1. Import the GPG key and check that its fingerprint is the one below.
2. Run `scripts/release-verify.sh <folder>`. It checks the signature of the
   list, the hash of every file, and each app's signing key against this
   page, and fails if any of them differs.

The builds are meant to be reproducible: `scripts/repro-check.sh` builds a
commit twice and compares the results. Nothing has been released yet, so no
key is listed.

## GPG key of the release list

fingerprint: (none yet)

## App signing keys

The SHA-256 of each app's signing certificate.

owner: (none yet)
counter: (none yet)
key: (none yet)
