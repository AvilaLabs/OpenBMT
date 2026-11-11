import { i18n } from './i18n'
import { sdk } from './sdk'
import {
  boxBind,
  boxPort,
  boxUid,
  dataDir,
  invitationFile,
  invitationPath,
  pageBind,
  pagePort,
} from './utils'

export const main = sdk.setupMain(async ({ effects }) => {
  /**
   * ======================== Setup ========================
   *
   * An invitation the action left in the volume is handed to the Box, which
   * reads the file once and deletes it (BX-12). It is read with `once()`, not
   * `const()`: the Box's own delete of the file must not restart the service.
   */
  console.info(i18n('Starting the Box'))
  const invitation = await invitationFile.read().once()

  /**
   * ======================== Daemons ========================
   *
   * One daemon, the Box without its console (--no-console). The image's
   * entrypoint is bypassed so the bind address is `[::]:8986` (BX-53), and
   * its status page is on `[::]:8987` (BX-61). The
   * command runs as the image's user, uid 1000. The volume's files are
   * stored under uid 0; the idmap shows them to the Box as uid 1000, so it
   * can write its key and settings with no chown step.
   */
  const args = [dataDir, boxBind, '--status-page', pageBind, '--no-console']
  if (invitation !== null) {
    args.push('--invitation-file', invitationPath)
  }

  return sdk.Daemons.of(effects)
    .addDaemon('box', {
      subcontainer: sdk.SubContainer.of(
        effects,
        { imageId: 'box' },
        sdk.Mounts.of().mountVolume({
          volumeId: 'main',
          subpath: null,
          mountpoint: dataDir,
          readonly: false,
          idmap: [{ fromId: 0, toId: boxUid, range: 1 }],
        }),
        'box-sub',
      ),
      exec: { command: ['/usr/local/bin/openbmt-box', ...args] },
      ready: {
        display: i18n('Relay'),
        fn: () =>
          sdk.healthCheck.checkPortListening(effects, boxPort, {
            successMessage: i18n('The Box is listening'),
            errorMessage: i18n('The Box is not listening'),
          }),
      },
      requires: [],
    })
    .addHealthCheck('page', {
      ready: {
        display: i18n('Status page'),
        fn: () =>
          sdk.healthCheck.checkPortListening(effects, pagePort, {
            successMessage: i18n('The status page is up'),
            errorMessage: i18n('The status page is not up'),
          }),
      },
      requires: ['box'],
    })
})
