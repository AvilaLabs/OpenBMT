import { FileHelper } from '@start9labs/start-sdk'
import { sdk } from './sdk'

// The port the Box listens on, inside the container and as published.
export const boxPort = 8986

// The Box listens on every IPv6 address, and with it, on Linux, every IPv4
// address (BX-53). StartOS reaches a raw TCP daemon in the client's own
// address family, so an IPv4-only listener would fail on every IPv6 address.
export const boxBind = `[::]:${boxPort}`

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
