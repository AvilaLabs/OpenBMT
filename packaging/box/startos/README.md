# OpenBMT Box for StartOS

A StartOS 0.4 package of the OpenBMT Box (`crates/openbmt-box`, rules in
`docs/BOX-RULES.md`). It is built with start-sdk 4.1.0 and is meant for
StartOS 0.4.2. It has not been run on a StartOS device (see the end).

This folder is part of the OpenBMT repository. Start9's registry takes a
package from a repository of its own, and its review makes a fork of that
repository the new upstream. Whether to move this folder into a repository
of its own, and when, is a release-time choice; nothing here depends on the
choice except the two paths in `startos/manifest/index.ts` that reach the
Box's Dockerfile from this folder.

## How it differs from the Box on a mini PC

- **Image.** The manifest builds the image from `packaging/box/Dockerfile`
  (`dockerBuild`, the repository root as the build context) for x86_64 and
  aarch64. No image is published anywhere. An aarch64 image built on an
  x86_64 machine runs the Rust build under emulation and takes a long time.
- **Entry.** The image's entrypoint is not used. The daemon's command is
  `openbmt-box /data [::]:8986 --status-page [::]:8987 --no-console`, run as
  the image's user (uid 1000), with `--invitation-file /data/invitation.txt`
  added when that file is in the volume.
- **IPv6.** A raw TCP binding on StartOS reaches the container in the
  client's own address family, so the Box listens on `[::]:8986`. On Linux
  that also accepts IPv4 (BOX-RULES BX-53, with its tests).
- **Volume ownership.** The one volume `main` is mounted at `/data` with an
  idmap (`fromId` 0, `toId` 1000): files stored under uid 0 on the volume
  appear as uid 1000 to the Box, which runs as uid 1000 and writes its key
  and settings there. The SDK has this mechanism (it needs StartOS 0.4.0
  beta 10 or later), so no chown step runs as root.
- **Stop.** StartOS stops the Box with SIGTERM. The Box installs no handler,
  so the default action ends it, and every file it writes is replaced whole
  (BX-50, BX-52). The test
  `bx_52_a_box_without_a_console_ends_at_sigterm_and_starts_again_with_its_key`
  in `crates/openbmt-box/tests/no_console.rs` sends SIGTERM to the binary
  and checks that it ends by that signal, in good time, and starts again on
  the same directory with the same key. The other tests in that file do the
  same with SIGKILL.
- **Invitation.** The Key app makes the invitation; the Box only takes it.
  It can be typed in the status page, or given by the action. The action
  "Enter invitation" takes the 30 characters, writes
  `invitation.txt` in the volume and restarts the Box, which reads the file
  once and deletes it (BX-12). The file is read at start with `once()`, so
  the Box's own delete of it does not restart the service.
- **Interfaces.** Two. `box` is a raw TCP interface on port 8986: no proxy,
  no TLS wrapper, `secure` with `ssl: false`. The Box's link seals and
  authenticates its messages itself. `page` is a `ui` interface for the
  Box's own status page (port 8987, protocol `http`, BX-54 to BX-61): StartOS
  serves it over HTTP and HTTPS on the addresses the user enables, and it
  opens from the service's page. It shows the Box's state in words and, while
  the Box waits, takes the invitation. StartOS shows the LAN addresses of
  both; a Tor address exists only if the user installs Tor and adds one. A
  second health check, "Status page", watches port 8987.
- **Backups.** The whole volume. StartOS stops the service for a backup.
- **Dependencies.** None. A Bitcoin Core dependency comes with the Box's own
  node.
- **Languages.** English only.

## Files

    startos/            the package source (manifest, main, interfaces,
                        actions, backups, versions, init)
    instructions.md     what the user reads on StartOS
    icon.svg            a plain geometric mark, a placeholder for a real logo
    Makefile            includes the SDK's s9pk.mk; x86_64 and aarch64
    package.json, package-lock.json
    tsconfig.json
    LICENSE             MIT, the repository's

`startos/index.ts`, `sdk.ts` and `i18n/index.ts` are the SDK template's
plumbing and are kept as they are.

## Checks that need no device

    npm ci
    npx tsc --noEmit
    npm run prettier:check

`npm ci` installs the SDK at the exact version in `package.json` and
everything else at the versions and hashes in `package-lock.json`.
`node_modules/` is git-ignored. CI runs the first two on every push
("Start9 package typecheck").

## Building and installing

With Docker (with a Buildx builder that has the OCI exporter), `make` and
`start-cli` installed, `make x86` or `make arm` in this folder builds the
`.s9pk`, and `make install` or the StartOS Sideload page installs it, as the
StartOS packaging guide describes. None of that has been run for this
package: no Docker and no `start-cli` were available where it was written.

## Not yet tested on a device

- That the `.s9pk` builds and installs.
- That the idmap makes `/data` writable by the Box on a real StartOS volume.
- That an IPv6 and an IPv4 client both reach the Box through the interface.
- That the "Status page" interface opens in a browser, on its LAN address and
  over HTTPS, and that StartOS passes the `Host` and `Origin` headers on as
  the browser sent them (the page refuses a form post whose `Origin` does not
  name its `Host`, BX-58, and says so in words if a proxy changes one).
- That the page takes an invitation and the Key app pairs the Box with it.
- That the "Enter invitation" action restarts the Box and the Box takes the
  invitation.
- That a backup and a restore bring the Box back with its key.
- That a StartOS stop ends the Box as the SIGTERM test says it should.
