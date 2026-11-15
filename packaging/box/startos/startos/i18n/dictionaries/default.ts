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
  'Status page': 11,
  'The status page is up': 13,
  'The status page is not up': 14,
  "The Box's state, and a box to type the invitation in": 12,

  // actions/enterInvitation.ts
  Invitation: 6,
  'The 30 characters the Key app shows when it makes an invitation': 7,
  'Letters, digits, spaces and hyphens only': 8,
  'Enter invitation': 9,
  'Give the Box the invitation from the Key app so that the Key app can pair it. The Box restarts.': 10,

  // interfaces.ts (the till)
  'Till address': 15,
  "The address the point-of-sale system's server gives for the Box's till listener": 16,

  // actions/posSetup.ts
  'POS setup': 17,
  'The key=value words the console command pos-setup takes, on one line, separated as that command says': 18,
  'One line': 19,
  'Give the Box the POS vendor setup. It is written as pos-setup.txt, and the Box reads and deletes it at its restart.': 20,

  // actions/posToken.ts
  'POS token': 21,
  'The read-only token from the point-of-sale vendor': 22,
  'Give the Box the POS vendor token. It is written as pos-token.txt, and the Box reads and deletes it at its restart. A Box that is not paired refuses it.': 23,

  // actions/tillListener.ts
  'Till listener address': 24,
  'Where the Box takes sale requests inside its container, as address:port; keep the port at 8091 so that the Till address interface reaches it': 25,
  'An IPv4 or IPv6 address, then a colon and the port': 26,
  "Turn the Box's till listener on. It is written as till.txt, which the Box keeps, and read at its restart.": 27,

  // actions/tillKey.ts
  'New till key': 28,
  'Make a new till key for the point-of-sale system and show it once. The old key stops working.': 29,
  'The key is shown only now. Copy it into the till system before closing the result.': 30,
  'Copy this key into the till system now. It is not shown again.': 31,
} as const

/**
 * Plumbing. DO NOT EDIT.
 */
export type I18nKey = keyof typeof dict
export type LangDict = Record<(typeof dict)[I18nKey], string>
export default dict
