export const DEFAULT_LANG = 'en_US'

const dict = {
  // main.ts
  'Starting the Box': 0,
  Relay: 1,
  'The Box is listening': 2,
  'The Box is not listening': 3,

  // interfaces.ts
  'Box address': 4,
  "The address the shop's devices give the Box as its relay": 5,

  // actions/enterInvitation.ts
  Invitation: 6,
  'The 30 characters the Key app shows when it makes an invitation': 7,
  'Letters, digits, spaces and hyphens only': 8,
  'Enter invitation': 9,
  'Give the Box the invitation from the Key app so that the Key app can pair it. The Box restarts.': 10,
} as const

/**
 * Plumbing. DO NOT EDIT.
 */
export type I18nKey = keyof typeof dict
export type LangDict = Record<(typeof dict)[I18nKey], string>
export default dict
