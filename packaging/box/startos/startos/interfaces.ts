import { i18n } from './i18n'
import { sdk } from './sdk'
import { boxPort, pagePort, tillPort } from './utils'

export const setInterfaces = sdk.setupInterfaces(async ({ effects }) => {
  const boxMulti = sdk.MultiHost.of(effects, 'box-multi')
  // Raw TCP, not HTTP: no proxy and no TLS wrapper. The Box's link seals and
  // authenticates its own messages, so the port is published as a plain port
  // with TLS disabled on every address the user enables.
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

  // The Box's status page (BX-54 to BX-62): plain HTTP inside, served by
  // StartOS over HTTP and HTTPS on the addresses the user enables. It shows
  // the Box's state and takes the invitation.
  const pageMulti = sdk.MultiHost.of(effects, 'page-multi')
  const pageOrigin = await pageMulti.bindPort(pagePort, {
    protocol: 'http',
  })
  const page = sdk.createInterface(effects, {
    name: i18n('Status page'),
    id: 'page',
    description: i18n("The Box's state, and a box to type the invitation in"),
    type: 'ui',
    masked: false,
    schemeOverride: null,
    username: null,
    path: '',
    query: {},
  })

  // The till listener (BX-70 to BX-80), raw TCP like the Box's own port: the
  // till's requests are signed and its answers are too, so the port needs no
  // TLS wrapper. The Box listens on it only after the action "Till listener
  // address" has written `till.txt` (BX-64).
  const tillMulti = sdk.MultiHost.of(effects, 'till-multi')
  const tillOrigin = await tillMulti.bindPort(tillPort, {
    protocol: null,
    addSsl: null,
    preferredExternalPort: tillPort,
    secure: { ssl: false },
  })
  const till = sdk.createInterface(effects, {
    name: i18n('Till address'),
    id: 'till',
    description: i18n(
      "The address the point-of-sale system's server gives for the Box's till listener",
    ),
    type: 'api',
    masked: false,
    schemeOverride: null,
    username: null,
    path: '',
    query: {},
  })

  return [
    await boxOrigin.export([box]),
    await pageOrigin.export([page]),
    await tillOrigin.export([till]),
  ]
})
