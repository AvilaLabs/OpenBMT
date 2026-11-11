import { sdk } from './sdk'

// The Box needs no other service. A Bitcoin Core dependency is added when the
// Box is given a node of its own.
export const dependencies = sdk.Dependencies.of()
