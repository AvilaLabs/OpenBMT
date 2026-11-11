import { i18n } from './i18n'
import { sdk } from './sdk'
import { boxPort, pagePort } from './utils'

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

  // The Box's status page (BX-54 to BX-61): plain HTTP inside, served by
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

  return [await boxOrigin.export([box]), await pageOrigin.export([page])]
})
