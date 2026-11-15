import { i18n } from '../i18n'
import { sdk } from '../sdk'
import { posSetupFile } from '../utils'

const { InputSpec, Value } = sdk

// The text the console's `pos-setup` takes: `key=value` words (BX-49). The
// Box checks it as the console does and says why in its log if it refuses it
// (BX-64); this only keeps out text that cannot be one.
const inputSpec = InputSpec.of({
  setup: Value.text({
    name: i18n('POS setup'),
    description: i18n(
      'The key=value words the console command pos-setup takes, on one line, separated as that command says',
    ),
    required: true,
    default: null,
    masked: false,
    minLength: 8,
    maxLength: 2048,
    patterns: [
      {
        regex: '^[^\\r\\n]+$',
        description: i18n('One line'),
      },
    ],
  }),
})

export const posSetup = sdk.Action.withInput(
  'pos-setup',
  {
    name: i18n('POS setup'),
    description: i18n(
      'Give the Box the POS vendor setup. It is written as pos-setup.txt, and the Box reads and deletes it at its restart.',
    ),
    warning: null,
    allowedStatuses: 'only-running',
    group: null,
    visibility: 'enabled',
  },
  inputSpec,
  async () => null,
  async ({ effects, input }) => {
    await posSetupFile.write(effects, `${input.setup.trim()}\n`)
    await effects.restart()
    return null
  },
)
