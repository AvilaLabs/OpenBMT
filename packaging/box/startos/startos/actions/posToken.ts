import { i18n } from '../i18n'
import { sdk } from '../sdk'
import { posTokenFile } from '../utils'

const { InputSpec, Value } = sdk

// The vendor's read-only token (BX-41). The Box refuses one that is not a
// token, or while it is not paired (BX-64), and says why in its log.
const inputSpec = InputSpec.of({
  token: Value.text({
    name: i18n('POS token'),
    description: i18n("The read-only token from the point-of-sale vendor"),
    required: true,
    default: null,
    masked: true,
    minLength: 1,
    maxLength: 1024,
    patterns: [
      {
        regex: '^[^\\r\\n]+$',
        description: i18n('One line'),
      },
    ],
  }),
})

export const posToken = sdk.Action.withInput(
  'pos-token',
  {
    name: i18n('POS token'),
    description: i18n(
      'Give the Box the POS vendor token. It is written as pos-token.txt, and the Box reads and deletes it at its restart. A Box that is not paired refuses it.',
    ),
    warning: null,
    allowedStatuses: 'only-running',
    group: null,
    visibility: 'enabled',
  },
  inputSpec,
  async () => null,
  async ({ effects, input }) => {
    await posTokenFile.write(effects, `${input.token}\n`)
    await effects.restart()
    return null
  },
)
