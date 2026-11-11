# Owner app: what the app itself tells a person

Made by `AppWords.screensMarkdown` in
`apps/owner-android/src/model/AppWords.kt`. Do not edit it by hand: run
`apps/owner-android/tests/run-jvm.sh` with the environment variable
`OPENBMT_WRITE_SCREENS=1` to write it again.

Everything about the coins, the payments, the queue, the link, the
old addresses and the refusals of the rules is shown with the words of
the library (`docs/screens/owner.md`, `docs/screens/owner-node.md`,
`openbmt-owner-ffi`). These are the words of what the app decides itself.

| State | Heading | What happened | What to do next | Who | Code |
|---|---|---|---|---|---|
| notSetUp | Not set up yet | This phone has not been set up as the shop's owner phone yet. | Paste the setup text from the key ceremony, or choose its file, then save. | the owner |  |
| setupSaved | Owner phone set up | The setup is saved on this phone, sealed with a key that stays in the phone. | Keep Orbot running and the app open. It starts looking for the network now. | the owner |  |
| setupNotReadable | Setup text not read | The setup text is not in the form this app reads, so nothing was saved. | Copy the setup text again from the key ceremony, then try again. | the owner |  |
| torAddressNotAccepted | Tor address not accepted | The address of Orbot must be a host, a colon and a port, like 127.0.0.1:9050. | Type the address again. | the owner |  |
| torAlwaysOn | Everything goes through Tor | This app sends every connection through Tor, with Orbot, and has no way to turn that off. | Check the address of Orbot below, and keep Orbot running. | the owner |  |
| orbotNotRunning | Tor is not running | This app sends everything through Tor, but Orbot is not running. | Open Orbot and turn it on. | the owner |  |
| notANumber | Check what you typed | An amount, a number or a rate is empty or has something other than digits in it. | Type whole numbers only, then try again. | the owner |  |
| counterNeeded | Counter needed | A refund is asked for on one counter's sale, and no counter is chosen. | Choose the counter, or paste its identity of 64 digits, then try again. | the owner |  |
| counterNotAccepted | Counter identity not read | A counter's identity is exactly 64 digits and letters from a to f. | Type the identity again, or choose the counter from the list. | the owner |  |
| responseNeeded | Response needed | The Key app's response is needed to finish the approval. | Paste the response or choose its file, then press the button again. | the owner |  |
| responseNotReadable | Response text not read | The text is not in the form this app reads, so the Key app's response was not used. | Copy the response again from the Key app, or choose its file instead. | the owner |  |
| requestReady | Request ready | The request for the Key app is ready, and nothing has been signed yet. | Copy it or save it as a file, give it to the Key app, then bring back its response. | the owner |  |
| requestCopied | Request copied | The request is copied as text, written in lower case. | Paste it where the Key app can read it. | the owner |  |
| requestSaved | Request saved | The request is saved as a file. | Carry the file to the Key app and open it there. | the owner |  |
| refundQueuedAtDepth | Refund queued | The refund is in the queue, and the sale is deep enough to be refunded. | Bring this phone and the Key app together and start an approval. | the owner |  |
| refundQueuedWaiting | Refund queued, sale new | The refund is in the queue, but the sale is not deep enough yet to be refunded. | Wait for more blocks. The refund can be approved when the sale is deep enough. | the owner |  |
| cashOutQueued | Cash-out queued | The cash-out is in the queue and has not been sent for approval yet. | Bring this phone and the Key app together and start an approval. | the owner |  |
| entryCancelled | Entry cancelled | The entry is taken out of the queue. | Nothing to do. | nobody |  |
| entryRequeued | Entry queued again | The entry that was sent back is in the queue again. | Start an approval when the Key app is at hand. | the owner |  |
| sessionAbandoned | Approval abandoned | The open approval is closed, and its entries are back in the queue. | Nothing to do. Start a new approval when the Key app is at hand. | nobody |  |
| alertCleared | Alert cleared | The alert is cleared from this phone. | Nothing to do. If the cause is still there, the alert comes back. | nobody |  |
| noteDismissed | Note dismissed | The note about a refund request is removed from the list. | Nothing to do. | nobody |  |
| noLink | No link to the counters | This phone was set up without a relay, so no counter can report to it. | Nothing to do here. A setup text with a relay adds the link. | the owner |  |
| noSettingsYet | No settings yet | The phone holds no settings for the shop yet, so there is nothing to show. | Nothing to do. They arrive with the first approval that carries settings. | the owner |  |
| starting | Starting | The owner node is starting and looking for the network. | Wait a moment. Keep the app open. | nobody |  |
| stopped | Owner node stopped | The owner node has stopped, so the wallet is no longer followed. | Close the app and open it again. | the owner |  |
| libraryMissing | The app is incomplete | The part of this app that runs the owner node did not load, so nothing was done. | Do not use this copy. Install the app again from the place you got it. | the owner |  |
| fault | Answer not readable | The app could not read an answer from the owner node, so nothing was changed. | Close the app and open it again. If it happens again, stop and do not spend. | the owner |  |
| storageUnavailable | Phone storage not usable | The phone's protected storage did not start, so the saved setup was not opened. | Restart the phone and open this app again. If it happens again, stop and do not spend. | the owner |  |
| fileNotRead | File not read | The file could not be opened or read, so nothing was done. | Pick the file again, or paste the text instead. | the owner |  |
| fileNotSaved | Not saved | The app could not write to this phone's storage, so nothing was saved. | Try again. If it happens again, free some space on the phone. | the owner |  |
