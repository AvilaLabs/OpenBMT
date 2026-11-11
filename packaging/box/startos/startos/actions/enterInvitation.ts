import { i18n } from '../i18n'
import { sdk } from '../sdk'
import { invitationFile } from '../utils'

const { InputSpec, Value } = sdk

// The Key app shows an invitation as 30 characters in six groups of five
// (BX-10). The Box reads it forgivingly (case, hyphens and spaces) and
// refuses a wrong check, so this only keeps out text that cannot be one.
const inputSpec = InputSpec.of({
  invitation: Value.text({
    name: i18n('Invitation'),
    description: i18n(
      'The 30 characters the Key app shows when it makes an invitation',
    ),
    required: true,
    default: null,
    masked: false,
    minLength: 26,
    maxLength: 40,
    patterns: [
      {
        regex: '^[0-9A-Za-z -]+$',
        description: i18n('Letters, digits, spaces and hyphens only'),
      },
    ],
  }),
})

export const enterInvitation = sdk.Action.withInput(
  'enter-invitation',
  {
    name: i18n('Enter invitation'),
    description: i18n(
      'Give the Box the invitation from the Key app so that the Key app can pair it. The Box restarts.',
    ),
    warning: null,
    allowedStatuses: 'only-running',
    group: null,
    visibility: 'enabled',
  },
  inputSpec,
  async () => null,
  async ({ effects, input }) => {
    // The file holds the invitation for the moments between this write and
    // the restart; the Box reads it once and deletes it. It is good for 30
    // minutes (BX-12).
    await invitationFile.write(effects, `${input.invitation.trim()}\n`)
    await effects.restart()
    return null
  },
)
