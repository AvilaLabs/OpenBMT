import { i18n } from '../i18n'
import { sdk } from '../sdk'
import { boxUid, dataDir } from '../utils'

// Makes a new till key with the Box's own `till-key` command (BX-71) and
// shows it once. The Box keeps the two keys made from it and not the key
// itself, so this is the only time it can be read. The Box that is running
// reads the new file at its next request; the old key stops at once.
export const tillKey = sdk.Action.withoutInput(
  'till-key',
  async () => ({
    name: i18n('New till key'),
    description: i18n(
      'Make a new till key for the point-of-sale system and show it once. The old key stops working.',
    ),
    warning: i18n(
      'The key is shown only now. Copy it into the till system before closing the result.',
    ),
    allowedStatuses: 'only-running',
    group: null,
    visibility: 'enabled',
  }),
  async ({ effects }) => {
    const out = await sdk.SubContainer.withTemp(
      effects,
      { imageId: 'box' },
      sdk.Mounts.of().mountVolume({
        volumeId: 'main',
        subpath: null,
        mountpoint: dataDir,
        readonly: false,
        idmap: [{ fromId: 0, toId: boxUid, range: 1 }],
      }),
      'till-key-sub',
      async (sub) =>
        sub.exec(['/usr/local/bin/openbmt-box', 'till-key', dataDir]),
    )
    const line = String(out.stdout)
      .split('\n')
      .find((l) => l.startsWith('till key: '))
    if (!line) {
      throw new Error(`The Box made no till key: ${String(out.stderr)}`)
    }
    return {
      version: '1',
      title: i18n('New till key'),
      message: i18n('Copy this key into the till system now. It is not shown again.'),
      result: {
        type: 'single',
        value: line.slice('till key: '.length).trim(),
        copyable: true,
        qr: false,
        masked: true,
        launchable: false,
      },
    }
  },
)
