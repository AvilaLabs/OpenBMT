// Where the page finds its files, as the repository holds them. The site build writes them as the site's (scripts/site-build.sh).
// A file of its own, not a script in the page: the site's content security policy runs no script written in a page.
globalThis.OpenBmtAssets = { logo: "../ui/logo.png", palette: "../ui/palette.json", model: "../../dist/kotlin-js/counter/counter-model.mjs", wasm: "../../dist/demo-counter/counter.wasm", host: "./counter-host.mjs",
  ownerModel: "../../dist/kotlin-js/owner/owner-model.mjs", ownerWasm: "../../dist/demo-owner/owner.wasm", ownerHost: "./owner-host.mjs",
  sounds: "./sounds/", logoPieces: "../ui/logo-pieces/" };
