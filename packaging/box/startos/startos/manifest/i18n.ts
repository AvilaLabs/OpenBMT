export const short = {
  en_US: "A shop's own relay between its bitcoin counters and its POS",
}

export const long = {
  en_US:
    "A small server for a shop that takes bitcoin with OpenBMT. It passes sealed messages between the shop's devices (the counters, the owner's phone and the Key app) and, where a connector is set up, matches the counters' paid notices to the point-of-sale system. It holds no bitcoin key, derives no address, checks no payment and cannot move funds. Whoever has its data has its identity key and any POS token it was given.",
}
