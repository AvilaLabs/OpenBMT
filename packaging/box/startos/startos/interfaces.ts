import { i18n } from './i18n'
import { sdk } from './sdk'
import { boxPort } from './utils'

export const setInterfaces = sdk.setupInterfaces(async ({ effects }) => {
  const boxMulti = sdk.MultiHost.of(effects, 'box-multi')
  // Raw TCP, not HTTP: no proxy and no TLS wrapper. The Box's link seals and
  // authenticates its own messages, so the port is published as a plain port
  // (secure with ssl false) on every address the user enables.
  const boxOrigin = await boxMulti.bindPort(boxPort, {
    protocol: null,
    addSsl: null,
    preferredExternalPort: boxPort,
    secure: { ssl: false },
  })
  const box = sdk.createInterface(effects, {
    name: i18n('Box address'),
    id: 'box',
    description: i18n(
      "The address the shop's devices give the Box as its relay",
    ),
    type: 'api',
    masked: false,
    schemeOverride: null,
    username: null,
    path: '',
    query: {},
  })

  return [await boxOrigin.export([box])]
})
