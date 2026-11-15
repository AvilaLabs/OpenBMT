import { FileHelper } from '@start9labs/start-sdk'
import { sdk } from './sdk'

// The port the Box listens on, inside the container and as published.
export const boxPort = 8986

// The port of the Box's status page inside the container (BX-54 to BX-62).
// StartOS serves it over HTTP and HTTPS as the interface "page".
export const pagePort = 8987

// The port of the till listener inside the container (BX-70 to BX-80). It is
// on only when `till.txt` names an address (BX-64); StartOS publishes the
// port as the raw TCP interface "till" either way.
export const tillPort = 8091

// The Box listens on every IPv6 address, and with it, on Linux, every IPv4
// address (BX-53). StartOS reaches a raw TCP daemon in the client's own
// address family, so an IPv4-only listener would fail on every IPv6 address.
export const boxBind = `[::]:${boxPort}`

// The status page listens the same way, so that it too is reached by an IPv6
// or an IPv4 client.
export const pageBind = `[::]:${pagePort}`

// Where the volume is mounted in the container, and the user the image runs
// as (see packaging/box/Dockerfile).
export const dataDir = '/data'
export const boxUid = 1000

// The file the Box reads once at start and deletes (BX-12). The action
// "Enter invitation" writes it; main.ts hands it to the Box if it is there.
export const invitationName = 'invitation.txt'
export const invitationPath = `${dataDir}/${invitationName}`

export const invitationFile = FileHelper.string({
  base: sdk.volumes.main,
  subpath: invitationName,
})

// The files the Box reads at start (BX-64). The actions below write them; the
// Box deletes `pos-setup.txt` and `pos-token.txt` once read, and keeps
// `till.txt`.
export const posSetupFile = FileHelper.string({
  base: sdk.volumes.main,
  subpath: 'pos-setup.txt',
})
export const posTokenFile = FileHelper.string({
  base: sdk.volumes.main,
  subpath: 'pos-token.txt',
})
export const tillFile = FileHelper.string({
  base: sdk.volumes.main,
  subpath: 'till.txt',
})
