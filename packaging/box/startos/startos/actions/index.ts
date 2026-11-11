import { sdk } from '../sdk'
import { enterInvitation } from './enterInvitation'

export const actions = sdk.Actions.of().addAction(enterInvitation)
