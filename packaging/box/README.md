# Packaging the Box as a container

The Box is the `openbmt-box` program (`crates/openbmt-box`, rules in
`docs/BOX-RULES.md`). This directory packages it as a container image for a
mini PC and for an Umbrel app. The image is built and tested in CI only.

## Files

- `Dockerfile`: two stages. The build stage is the official Rust image
  (1.89, the workspace minimum) and runs
  `cargo build --locked --release -p openbmt-box`. The runtime stage is
  Debian slim with the binary, `ca-certificates` and `curl`. Both base images
  are pinned by digest, with the tag and the date it was read beside each.
- `.dockerignore` (at the repository root): keeps `target/`, `.git`, the
  fuzz corpora, `docs/`, `apps/` and `scripts/` out of the build context.
- `compose.yaml`: one service for a mini PC. The port is published on the
  LAN address given in `OPENBMT_BOX_LAN_ADDR` only, the data is a volume, the
  root file system is read-only, `no-new-privileges` is set, every
  capability is dropped and memory is limited.
- `umbrel/umbrel-app.yml` and `umbrel/docker-compose.yml`: an app in the
  Umbrel app-store format. Not submitted anywhere.

## Build and run

From the repository root:

    docker build -f packaging/box/Dockerfile -t openbmt-box .
    OPENBMT_BOX_LAN_ADDR=192.168.1.20 \
      docker compose -f packaging/box/compose.yaml up -d

Or with docker alone:

    docker volume create box-data
    docker run -d -i --init --read-only --cap-drop ALL \
      --security-opt no-new-privileges -v box-data:/data \
      -p 192.168.1.20:8986:8986 openbmt-box

Two things the Box needs from whatever starts it:

- Standard input open (`-i`, `stdin_open`). The Box reads its console from
  standard input and stops when it ends. The invitation (BX-12) is typed
  there: `docker attach <container>`, then `invite <text>`.
- An init (`--init`, `init: true`). The Box has no handler for SIGTERM, and
  a process that is the container's first has no default action for it.

The address the Box listens on inside the container is `OPENBMT_BOX_BIND`
(default `0.0.0.0:8986`). The health check is a TCP connect to that port; the
Box has no health answer of its own.

## What the container holds

The binary, `ca-certificates` and `curl`, and the volume `/data` (owned by
uid and gid 1000, mode 0700), where the Box keeps what BOX-RULES sections 1
and 5 say it keeps: its identity key, the settings in force, its members and
mailboxes and, if the owner gives it one, its POS token. It runs as uid 1000,
not root. It holds no shop key and no wallet description (BX-04). `curl` is
installed for the POS connectors, which reach the vendors over HTTPS; the
Box does not use it yet.

Whoever has the volume has the Box's key and POS token (BOX-RULES section 6).
Back the volume up as one would a password file.

## To verify on an Umbrel device

None of the Umbrel files has been run on an Umbrel device.

- `manifestVersion: 1` and every field of `umbrel-app.yml` (the category,
  `gallery`, `path`, `releaseNotes`, `submitter`, `submission`, and whether
  the version must be a string).
- The `app_proxy` variables (`APP_HOST`, `APP_PORT`) and the container name
  `openbmt-box_server_1`. The proxy speaks HTTP; the Box's port speaks the
  Box's own framing over TCP, so the proxy may not be the way to reach it.
- `restart: on-failure`, `stdin_open` and `init` under Umbrel's runner, which
  has no terminal to type the invitation into: `--invitation-file` may be the
  only way in.
- That `${APP_DATA_DIR}/data` can be written by uid 1000.
- `image: openbmt-box:0.1.0`: a published image, pinned by digest, is needed;
  none is published.

## Not done

- A Start9 package. Its current SDK format was not verified.
- A mini PC disk image.
- Push wake-ups.
- A full node beside the Box.
- Updates outside trading hours.
- Any POS connector: the program holds none.
