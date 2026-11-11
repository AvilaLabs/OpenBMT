# Packaging the Box

The Box is the `openbmt-box` program (`crates/openbmt-box`, rules in
`docs/BOX-RULES.md`). This directory packages it three ways: as a container
image with a compose file for a mini PC, as an Umbrel app and as a StartOS
package. The image is built and smoke-tested in CI. Neither the Umbrel files
nor the StartOS package has been run on a device.

## Files

- `Dockerfile`: two stages. The build stage is the official Rust image
  (1.89, the workspace minimum) and runs
  `cargo build --locked --release -p openbmt-box`. The runtime stage is
  Debian slim with the binary, `ca-certificates` and `curl`. Both base images
  are pinned by digest, with the tag and the date it was read beside each.
- `.dockerignore` (at the repository root): keeps `target/`, `.git`, the
  fuzz corpora, `docs/`, `apps/` and `scripts/` out of the build context.
- `compose.yaml`: one service for a mini PC.
- `umbrel/`: an app in the Umbrel app-store format.
- `startos/`: a StartOS 0.4 package, with its own README.

## Docker compose

What works: the image builds, starts, answers on its port, passes its health
check, ends on `docker stop` and starts again with its key (CI runs exactly
that). The compose file publishes the port on the LAN address given in
`OPENBMT_BOX_LAN_ADDR` only, keeps the data in a volume, makes the root file
system read-only, sets `no-new-privileges`, drops every capability and
limits memory.

To try it, from the repository root:

    docker build -f packaging/box/Dockerfile -t openbmt-box .
    OPENBMT_BOX_LAN_ADDR=192.168.1.20 \
      docker compose -f packaging/box/compose.yaml up -d

Or with docker alone:

    docker volume create box-data
    docker run -d --init --read-only --cap-drop ALL \
      --security-opt no-new-privileges -v box-data:/data \
      -p 192.168.1.20:8986:8986 openbmt-box

The image starts the Box with `--no-console`: it reads no standard input and
runs until a signal ends it, so the container needs no terminal. An
invitation (BX-12) is given as a file: put it in the volume and start the
container with the extra arguments `--invitation-file /data/invitation.txt`.
The Box reads the file once and deletes it.

The Box can also serve a status page: the extra arguments
`--status-page 0.0.0.0:8987` turn it on (it is off by default; BX-54 to
BX-61) and the port is published like the Box's own, for example
`-p 192.168.1.20:8987:8987`. The page is plain HTTP with no login of its own,
so on a mini PC publish it on the LAN address only, as the compose file does
for the relay, and do not leave it on a network others use. The compose file
for a mini PC does not turn it on.

The Box installs no signal handler, so `docker stop` ends it by the default
action of SIGTERM. That loses nothing: every file the Box writes is replaced
whole, and a log's torn tail is cut at the next start (BX-50, BX-52; the
Box's tests kill it with SIGKILL and with SIGTERM at several points and start
it again). The Box does need an init (`--init`, `init: true`): a process
that is the container's first has no default action for SIGTERM. A stopped
container exits with 143 (128 and 15), not 0.

The address the Box listens on inside the container is `OPENBMT_BOX_BIND`
(default `0.0.0.0:8986`). It may be an IPv6 address such as `[::]:8986`
(BX-53). The health check is a TCP connect to that port; the Box has no
health answer of its own.

The image holds the binary, `ca-certificates` and `curl`, and the volume
`/data` (owned by uid and gid 1000, mode 0700), where the Box keeps what
BOX-RULES sections 1 and 5 say it keeps: its identity key, the settings in
force, its members and mailboxes and, if the owner gives it one, its POS
token. It runs as uid 1000, not root. It holds no shop key and no wallet
description (BX-04). `curl` is installed for the POS connectors, which reach
the vendors over HTTPS; the Box does not use it yet.

Whoever has the volume has the Box's key and POS token (BOX-RULES section 6).
Back the volume up as one would a password file.

Not tested on a device: a mini PC with a real shop's devices.

## Umbrel

What works: the files follow the current umbrel-apps rules for the manifest
(field order, `storage` with `dataRoot: data`, empty `defaultUsername` and
`defaultPassword`, a first-run note at the top of the description) and the
compose file (data under `${APP_DATA_DIR}/data` with a committed
`data/.gitkeep`, the Box run as uid 1000 with an init).

The app opens to a page. The Box serves its own status page (BX-54 to
BX-61, `--status-page 0.0.0.0:8987`, passed by the compose file) and the
compose file has an `app_proxy` that routes the manifest's `port: 8987` to
it (`APP_HOST: openbmt-box_server_1`, `APP_PORT: 8987`). Umbrel's login is
left on. The page says whether the Box is waiting for an invitation, has one
open, is paired (with the shop's four short words) or is turned off, each
with what it means and what to do next; it gives the address the shop's
devices use (the host the visitor typed and port 8986), the version and how
many devices are on the list; and while the Box waits it has a box for the
invitation. The compose file also still maps the Box's raw port as
`8986:8986/tcp` on the host, and the devices connect to that directly: they
do not go through the proxy or the login. An invitation can still be given
as the file `invitation.txt` in the app's data folder
(`app-data/openbmt-box/data`), read at the next start.

How the port was chosen. The manifest `port` must be unique across the App
Store, and `8987` is the port the page listens on inside the container. The
umbrel-apps tree (`getumbrel/umbrel-apps`, commit `3a08738`, read on
2026-11-11 as a tarball of its `umbrel-app.yml` and `docker-compose.yml`
files, 399 apps) has no manifest with `port: 8987` or `8986`, no compose
file that publishes either, and no `APP_PORT` of either; its manifest ports
near these are 8985, 8989 and 8980. The check is a read of that tree at one
commit: another app may take the port before this one is submitted, so it is
repeated then.

To try it, copy `umbrel/` into a community app store or an umbrel-apps
checkout as a folder named `openbmt-box`, with an image named in
`docker-compose.yml` that the Umbrel can pull.

Not yet tested on an Umbrel device: that the app installs, starts and keeps
its data, that uid 1000 can write the data folder, that restarts and updates
keep the key, that the manifest passes the umbrel-apps linter, that the
page opens through Umbrel's proxy with its login (and that the proxy passes
the `Host` and `Origin` headers as the browser sent them, which the page's
cross-site check (BX-58) needs: a proxy that rewrites `Host` makes the page
refuse the invitation form with words), and that the form takes an
invitation there. The page itself, its headers, its refusals and its
invitation form are tested on the loopback (the Box's tests and
`scripts/box-e2e.sh`, step 17).

What now meets the App Store's rule that an app opens to a useful page, and
what does not yet:

- Met in the files: the app opens to a status and setup page that shows the
  Box's state and address and takes the invitation, behind `app_proxy` on a
  manifest `port` other than 8986, so that nobody needs a shell on the
  Umbrel.
- Release time: the image line is `openbmt-box:0.1.0`, a local name. The App
  Store needs a published image for linux/amd64 and linux/arm64, pinned as
  `registry/repo:version@sha256:<index digest>`. No image is published; that
  is done when a release is made.
- Not done: the gallery and the icon, which the Umbrel team adds; the
  manifest `submission`, which is empty because nothing is submitted.

## Start9

What works: a StartOS 0.4 package in `startos/` (start-sdk 4.1.0): the
manifest with an image built from the Box's Dockerfile for x86_64 and
aarch64, two interfaces (the Box's raw TCP port and a `ui` interface for
its status page), the daemon with a listening health check for each, the
data volume and its backup, and the action "Enter invitation". The Box is
started with
`--status-page [::]:8987` and StartOS serves that port as the interface
"Status page" over HTTP and HTTPS on the addresses the user enables. It
typechecks, and CI typechecks it on every push. Both ways to give the
invitation work: the page, and the action.

To try the checks, from `startos/`: `npm ci`, `npx tsc --noEmit`. To build
and install the package, see `startos/README.md`; that needs Docker and
`start-cli`.

Not yet tested on a StartOS device: that the package builds and installs,
volume ownership under the idmap, IPv6 and IPv4 clients, that the "Status
page" interface opens in the browser and that StartOS passes the `Host` and
`Origin` headers on (the page's cross-site check, BX-58, needs them), the
invitation page and the invitation action, backup and restore, and stopping
the service. The list is in
`startos/README.md`. No image is published and nothing is submitted to
Start9's registry.

## Not done

- A mini PC disk image.
- Push wake-ups.
- A full node beside the Box.
- Updates outside trading hours.
- Any POS connector: the program holds none.
