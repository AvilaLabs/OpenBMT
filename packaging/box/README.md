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
`data/.gitkeep`, the Box run as uid 1000 with an init, a raw TCP port
mapping).

The Box has no web page, so there is no `app_proxy`: the compose file maps
the Box's port as `8986:8986/tcp` on the host, and the devices connect to
it directly. An invitation is given by saving it as `invitation.txt` in the
app's data folder (`app-data/openbmt-box/data` under the Umbrel's data
root) and restarting the app; the compose file passes
`--invitation-file /data/invitation.txt` to the Box. With no such file the
Box says so in its log and runs on.

To try it, copy `umbrel/` into a community app store or an umbrel-apps
checkout as a folder named `openbmt-box`, with an image named in
`docker-compose.yml` that the Umbrel can pull.

Not yet tested on an Umbrel device: that the app installs, starts and keeps
its data, that uid 1000 can write the data folder, that restarts and updates
keep the key, and that the manifest passes the umbrel-apps linter. Two things
stand between these files and the official App Store, which they are not
submitted to:

- The App Store requires an app to open to a web page (a status or setup
  page). The Box has none. The package needs one, served behind `app_proxy`
  on a manifest `port` other than 8986, that shows the Box's state and its
  address and takes the invitation, so that nobody needs a shell on the
  Umbrel.
- The image line is `openbmt-box:0.1.0`, a local name. The App Store needs a
  published image for linux/amd64 and linux/arm64, pinned as
  `registry/repo:version@sha256:<index digest>`. No image is published; that
  is done when a release is made.

`submission` in the manifest is empty because nothing is submitted.

## Start9

What works: a StartOS 0.4 package in `startos/` (start-sdk 4.1.0): the
manifest with an image built from the Box's Dockerfile for x86_64 and
aarch64, one raw TCP interface, the daemon with a listening health check,
the data volume and its backup, and the action "Enter invitation". It
typechecks, and CI typechecks it on every push.

To try the checks, from `startos/`: `npm ci`, `npx tsc --noEmit`. To build
and install the package, see `startos/README.md`; that needs Docker and
`start-cli`.

Not yet tested on a StartOS device: that the package builds and installs,
volume ownership under the idmap, IPv6 and IPv4 clients, the invitation
action, backup and restore, and stopping the service. The list is in
`startos/README.md`. No image is published and nothing is submitted to
Start9's registry.

## Not done

- A mini PC disk image.
- Push wake-ups.
- A full node beside the Box.
- Updates outside trading hours.
- Any POS connector: the program holds none.
