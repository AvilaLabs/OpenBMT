import { sdk } from '../sdk'
import { enterInvitation } from './enterInvitation'
import { posSetup } from './posSetup'
import { posToken } from './posToken'
import { tillKey } from './tillKey'
import { tillListener } from './tillListener'

export const actions = sdk.Actions.of()
  .addAction(enterInvitation)
  .addAction(posSetup)
  .addAction(posToken)
  .addAction(tillListener)
  .addAction(tillKey)
