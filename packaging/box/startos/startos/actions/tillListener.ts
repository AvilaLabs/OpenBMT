import { i18n } from '../i18n'
import { sdk } from '../sdk'
import { tillFile, tillPort } from '../utils'

const { InputSpec, Value } = sdk

// `listen=<addr:port>` in till.txt turns the till listener on (BX-64). The
// port is the one StartOS publishes as the interface "till".
const inputSpec = InputSpec.of({
  listen: Value.text({
    name: i18n('Till listener address'),
    description: i18n(
      'Where the Box takes sale requests inside its container, as address:port; keep the port at 8091 so that the Till address interface reaches it',
    ),
    required: true,
    default: `[::]:${tillPort}`,
    masked: false,
    minLength: 3,
    maxLength: 64,
    patterns: [
      {
        regex: '^[0-9A-Fa-f.:\\[\\]]+$',
        description: i18n('An IPv4 or IPv6 address, then a colon and the port'),
      },
    ],
  }),
})

export const tillListener = sdk.Action.withInput(
  'till-listener',
  {
    name: i18n('Till listener address'),
    description: i18n(
      'Turn the Box\'s till listener on. It is written as till.txt, which the Box keeps, and read at its restart.',
    ),
    warning: null,
    allowedStatuses: 'only-running',
    group: null,
    visibility: 'enabled',
  },
  inputSpec,
  async () => null,
  async ({ effects, input }) => {
    await tillFile.write(effects, `listen=${input.listen.trim()}\n`)
    await effects.restart()
    return null
  },
)
