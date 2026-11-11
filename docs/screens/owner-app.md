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
| notSetUp | Not set up yet | This phone has not been set up as the shop's owner phone yet. | Make a new shop wallet with the Key app, or use setup text you already have. | the owner |  |
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
| requestReady | Request ready | The request for the Key app is ready, and nothing has been signed yet. | Show it as a code, or save it as a file or copy it as text, give it to the Key app, then bring back its response. | the owner |  |
| requestCopied | Request copied | The request is copied as text, written in lower case. | Paste it where the Key app can read it. | the owner |  |
| requestCodeAnimated | Request shown as a code | The request is on the screen as a code that changes, one part after another. | On the Key app, choose Scan and point it at this code until it has read every part. Then press Done. | the owner |  |
| requestCodeStill | Request shown as one code | The request is on the screen as one code that does not change. | On the Key app, choose Scan and point it at this code. Then press Done. | the owner |  |
| requestCodeNotMade | No code could be made | The request could not be turned into a code, so nothing is shown. | Go back and save the request as a file, or copy it as text, instead. | the owner |  |
| scanCancelled | Scan cancelled | No code was read, so nothing was taken from the Key app. | Scan the Key app's code again, or choose its file or paste its text instead. | the owner |  |
| scanNotAResponse | Not the Key app's code | The code read is not one the Key app shows for a response, so nothing was used. | On the Key app, show the answer as a code, and scan that one. | the owner |  |
| counterReading | Reading the counter | The counter's request is being read and its four words worked out, which takes a moment. | Wait on this screen. | the owner |  |
| counterNotACode | Not a counter's code | The code read is not one a counter shows for its request, so nothing was queued. | On the counter, choose Pair and scan the code it shows. | the owner |  |
| counterScanCancelled | Counter scan cancelled | No code was read, so nothing was queued. | Scan the counter's code again when you are ready. | the owner |  |
| counterCodeNotRequest | Not a counter's request | The code holds more than a counter's request can, so nothing was queued. | On the counter, choose Pair and scan the code it shows. | the owner |  |
| sharesNote | Equal shares | Each counter's share of the instant limit becomes 1/2. | Custom shares come later, with the settings editor. | the owner |  |
| sharesNoteOne | Equal shares | This is the only counter, so its share of the instant limit is all of it. | Custom shares come later, with the settings editor. | the owner |  |
| counterCompare | Compare the four words | Check that the counter shows these same four words, and that the Key app shows them too. | If they match, say so below. If they differ, say so and nothing is paired. | the owner |  |
| counterRateNeeded | Rate needed | This phone holds no settings yet, and the starting limits are in dollars, so it needs today's rate. | Type satoshis to a dollar as a whole number above 0, then say the words match. | the owner |  |
| counterCancelled | Counter not added | The four words did not match, so the counter's request was taken out of the queue. | Start again on the counter, and scan the new code it shows. | the owner |  |
| counterNotPaired | Counter not paired | The Key app did not approve this counter, so no code for it was made. | Read the words above, then scan the counter's code again to start over. | the owner |  |
| counterCodeAnimated | Code for the counter | The code for the counter is on the screen in many parts, with its first addresses. Taking it takes about a minute. | Scan this with the counter and hold both phones steady. The counter counts the parts it has and shows Paired when it has them all. | the owner |  |
| counterCodeStill | Code for the counter | The code for the counter is on the screen as one code that does not change. | Scan this with the counter. It shows Paired when it has taken it. | the owner |  |
| counterCodeNoBatch | Code without addresses | The first addresses are not in this code, because the link handed them over already. | Scan this with the counter. It shows Paired when it has taken it. | the owner |  |
| counterCodeNotMade | No code could be made | The code for the counter could not be made, so nothing is shown. What waits for the counter is kept. | Try to show it again. | the owner |  |
| counterDelivered | Counter paired | The counter shows Paired, so what waited for it is marked delivered. | The counter takes its sales from its first addresses now. | the owner |  |
| ceremonyMakingKey | Making key 1 | Key 1 is being made on this phone, which takes a moment. | Wait on this screen. | the owner |  |
| ceremonyScanNext | Scan the Key app | Open the Key app on the other phone and choose Set up keys. | Press the button below and scan the code it shows. | the owner |  |
| ceremonyNotAnAnswer | Not the Key app's answer | The code read is not one the Key app shows for its answer, so nothing was used. | On the Key app, choose Set up keys, and scan the code it shows. | the owner |  |
| ceremonyCodeTooLong | Code too long for an answer | The code holds more than the Key app's answer can, so nothing was used. | On the Key app, choose Set up keys, and scan the code it shows. | the owner |  |
| ceremonyScanCancelled | Key app scan cancelled | No code was read, so no answer was taken from the Key app. | Scan the Key app's code again when you are ready. | the owner |  |
| ceremonyCompareRoot | Compare the Key app's words | Check that the Key app shows these same four words as its own. | If they match, say so below. If they differ, say so and the answer is thrown away. | the owner |  |
| ceremonyRootDiffers | Answer thrown away | The four words differ, so this answer may not come from the Key app you hold, and it was thrown away. | Scan again on the Key app you hold. If the words still differ, choose Start again. | the owner |  |
| ceremonyKey2Needed | Key 2 needed | The Key app holds no key 2, so key 2 comes from the device that holds it. | Scan its code or paste its text, then continue. | the owner |  |
| ceremonyKey2Text | Key 2 text not accepted | The text for key 2 is empty, is not text, or is longer than a key's text can be, so nothing was used. | Copy it again from the device that holds key 2, then try again. | the owner |  |
| ceremonyKey2ScanCancelled | Key 2 scan cancelled | No code was read, so key 2 was not taken. | Scan its code again, or paste its text instead. | the owner |  |
| ceremonyBuilding | Building the wallet | The wallet is being built from the three keys, which takes a moment. | Wait on this screen. | the owner |  |
| ceremonyCheckWallet | Check the wallet | The checksum words and the fingerprints below are the wallet's. The Key app shows the same ones. | Show the Key app its setup next, and compare there. | the owner |  |
| ceremonyCodeAnimated | Setup for the Key app | The setup for the Key app is on the screen as a code that changes, one part after another. | On the Key app, scan this code until it has read every part, then compare the words. | the owner |  |
| ceremonyCodeStill | Setup for the Key app | The setup for the Key app is on the screen as one code that does not change. | On the Key app, scan this code, then compare the words. | the owner |  |
| ceremonyCodeNotMade | No code could be made | The setup for the Key app could not be turned into a code, so nothing is shown. | Go back and try to show it again. | the owner |  |
| ceremonyCompareSetup | Compare with the Key app | After it takes the setup, the Key app shows the checksum words and the fingerprints. | Check that they are the same as on this phone, then say so below. | the owner |  |
| ceremonyWordsDiffer | Wallet not finished | The words differ, so the wallet is not finished, and nothing should be paid to it. | Choose Start again on both phones. | the owner |  |
| ceremonyScanAgain | Scan the Key app again | The wallet was built, but the setup for the Key app is not kept on this phone. | Scan the Key app's code again to show the setup again. | the owner |  |
| ceremonyFinishing | Finishing the setup | The setup is being finished and saved on this phone. | Wait on this screen. | the owner |  |
| ceremonySaveNext | Save the setup | The words were confirmed, but the setup was not saved on this phone yet. | Press the button below to save it and start. | the owner |  |
| ceremonyResume | Setup in progress | A new shop wallet was started on this phone and is not finished. | Continue where it stopped, or start again. | the owner |  |
| ceremonyStartAgain | Start again? | The keys made so far are thrown away, and nothing has been paid to them. | Choose Start again only if the Key app starts again too. | the owner |  |
| ceremonyThrownAway | Started over | The keys made so far were thrown away, and nothing was paid to them. | Make a new shop wallet again, or use setup text you already have. | the owner |  |
| ceremonyRecordNotRead | Setup record not read | The record of the unfinished setup could not be read, so nothing was done. | Choose Start again and make the wallet again. Nothing has been paid to it. | the owner |  |
| setupTextNeeded | Setup text needed | This phone takes the setup text of a key ceremony made elsewhere, pasted or from a file. | Paste the setup text, or choose its file, then save. | the owner |  |
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
| posNotConnected | No till connected | This phone is not reading any till, so no order is matched against the counters' sales. | Choose the till's vendor, type its location, and paste a read-only sign-in, then connect. | the owner |  |
| posNeedsLink | Till needs the link | Matching a till needs the link to the counters, and this phone was set up without a relay. | Nothing to do here. A setup text with a relay adds the link. | the owner |  |
| posTokenNotAccepted | Sign-in not accepted | A read-only sign-in is 1 to 512 letters, digits and signs, with no space in it. | Paste it again from the till's system, or make a new one there. | the owner |  |
| posConnected | Till connected | The till is connected, and its sign-in is sealed on this phone and is not shown again. | Nothing to do. Keep the app open: the till is read about once a minute. | nobody |  |
| posRemoved | Till taken off | The till's settings, its sign-in and the matches kept for it are removed from this phone. | Nothing to do. Connect a till again when you want its orders matched. | nobody |  |
| posAcknowledged | Alert acknowledged | The alert is acknowledged on this phone only: the Box and the counters do not learn of it. | Nothing to do. | nobody |  |
| posTokenMissing | Sign-in missing | A till is connected, but this phone holds no sign-in for it, so nothing is read. | Replace the sign-in with a new read-only one from the till's system. | the owner |  |
| posAddressRefused | Till address refused | The library asked for an address that is not the till's system, so nothing was sent. | Close the app and open it again. If it happens again, do not rely on the matching. | the owner |  |
| posPageTooLong | Till answer too long | The till's system sent an answer longer than 8 megabytes, so it was not read. | Nothing to do now. The next read is in about a minute. | the owner |  |
| posReadCutOff | Till read cut off | One read asked for more pages than a read may take, so it was ended. | Nothing to do now. The next read is in about a minute. | the owner |  |
| posNotReached | Till not reached | The till's system did not answer through Tor, so nothing was read. | Check that Orbot is running and the phone is online. The next read is in about a minute. | the owner |  |
| starting | Starting | The owner node is starting and looking for the network. | Wait a moment. Keep the app open. | nobody |  |
| stopped | Owner node stopped | The owner node has stopped, so the wallet is no longer followed. | Close the app and open it again. | the owner |  |
| libraryMissing | The app is incomplete | The part of this app that runs the owner node did not load, so nothing was done. | Do not use this copy. Install the app again from the place you got it. | the owner |  |
| fault | Answer not readable | The app could not read an answer from the owner node, so nothing was changed. | Close the app and open it again. If it happens again, stop and do not spend. | the owner |  |
| storageUnavailable | Phone storage not usable | The phone's protected storage did not start, so the saved setup was not opened. | Restart the phone and open this app again. If it happens again, stop and do not spend. | the owner |  |
| fileNotRead | File not read | The file could not be opened or read, so nothing was done. | Pick the file again, or paste the text instead. | the owner |  |
| fileNotSaved | Not saved | The app could not write to this phone's storage, so nothing was saved. | Try again. If it happens again, free some space on the phone. | the owner |  |
| cameraOff | Camera is off for this app | This app is not allowed to use the camera, so it cannot scan a code. | Allow the camera for this app in the phone's settings, or paste the text or choose a file instead. | the owner |  |
| cameraNotStarted | Camera did not start | The camera could not be opened, so nothing was scanned. | Close any other app that uses the camera and try again, or paste the text or choose a file instead. | the owner |  |
| UrWords.reading | Reading the animated code | Reading the animated code: 3 of about 9 parts. | Keep the phone pointed at the code until it is done. | the owner |  |
| UrWords.restarted | Code did not add up | The parts read did not fit together, so reading starts again. | Keep the phone pointed at the code. It reads from the first part again. | the owner |  |
| UrWords.otherCode | Another code is in view | The code in view is not part of the one being read, so it is skipped. | Point the phone at the first code again, or cancel and start over. | the owner |  |
| UrWords.wrongKind | Not the code expected | This code carries something this step does not take, so nothing was used. | Cancel, then ask for the right code again, or use a file instead. | the owner |  |
| UrWords.contentRefused | Code content not usable | The code was read in full, but what it holds is not in a form this step takes. | Ask for the code to be shown again, or use a file instead. | the owner |  |
| UrWords.readerFailed | Code reader failed | The part of this app that reads codes gave no usable answer, so nothing was read. | Close the app and open it again. If it happens again, use a file. | the owner |  |
