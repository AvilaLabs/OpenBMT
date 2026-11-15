import { sdk } from './sdk'

// BX-63: the till key and the POS token are left out of backups, so a Box
// restored from an old backup does not bring back a key or a token replaced
// since; they are set up again after a restore. `pos-token.txt` and its
// `.refused` copy (BX-64) hold a token too.
export const { createBackup, restoreInit } = sdk.setupBackups(async () =>
  sdk.Backups.ofVolumes('main').setOptions({
    exclude: [
      'till-key',
      'pos.token',
      'pos-token.txt',
      'pos-token.txt.refused',
    ],
  }),
)
