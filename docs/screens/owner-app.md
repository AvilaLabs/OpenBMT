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
| sharesNote | Equal shares | Each counter's share of the instant limit becomes 1/2. | Custom shares are set under Settings, in Change settings. | the owner |  |
| sharesNoteOne | Equal shares | This is the only counter, so its share of the instant limit is all of it. | Custom shares are set under Settings, in Change settings. | the owner |  |
| counterCompare | Compare the four words | Check that the counter shows these same four words, and that the Key app shows them too. | If they match, say so below. If they differ, say so and nothing is paired. | the owner |  |
| counterPriceNeeded | Price needed | This phone holds no settings yet, and the starting limits are in dollars, so it needs today's price of a bitcoin. | Type today's price of one bitcoin in whole dollars, then say the words match. | the owner |  |
| priceNotRead | Price not read | The price of a bitcoin is a whole number of dollars above 0, so nothing was queued. | Type today's price of one bitcoin in whole dollars, like 98500. | the owner |  |
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
| ceremonyKey2Needed | Key 2 needed | The Key app holds no key 2, so key 2 comes from the device that holds it. | Scan a key code from the other wallet app, or its key as text, or paste the text, then continue. | the owner |  |
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
| editIntro | Change the settings | Pick a group, change what you need, then review. Nothing is queued until you press Queue the change. | A change that loosens something needs the Key app. A stricter change does not. | the owner |  |
| editGroupInstant | Instant sales | How much a counter may accept as paid before the payment is confirmed, for each way it checks. | Whole dollars. 0 turns a limit off. | the owner |  |
| editGroupPayments | Payments | When a payment counts, how many blocks settle a sale, and the smallest bitcoin sale. | Change what you need, then press Done. | the owner |  |
| editGroupRefunds | Refunds and overrides | How much can be refunded, and how much cashiers and managers may accept against a red screen. | Whole dollars. 0 turns a limit off. | the owner |  |
| editGroupSafety | Safety | How recent this phone's report must be, and what the shop says about its network. | Change what you need, then press Done. | the owner |  |
| editGroupShares | Counters' shares | Each counter's share of every instant limit. The shares may add up to 100% at most. | Type each share as a percent with up to two decimals. | the owner |  |
| editGroupLightning | Lightning node | The shop's own Lightning node: every Lightning payment must be made out to this node's key. | Paste or scan the node's key or address, or remove the node. | the owner |  |
| editLightningNote | Naming a node | Naming a node needs the Key app, which shows the key in full. | Afterwards, set up Lightning on each counter, which holds its own limited key. | the owner |  |
| editNothingChanged | Nothing changed | Every value is what the settings hold now, so there is nothing to queue. | Go back and change a value, or leave the editor. | the owner |  |
| editNeedsKeyApp | The Key app must approve | Something here loosens a limit or a check, so the whole change needs the Key app's approval. | Queue the change, then build the request for the Key app. | the owner |  |
| editTakesEffect | Takes effect when delivered | Every change here makes things stricter, so this phone signs it alone and no approval is needed. | Queue the change. It takes effect when it is delivered to the counters. | the owner |  |
| editQueuedLoosening | Change queued | The change waits for the Key app's approval. It takes effect after the Key app approves and it is delivered. | Build the request for the Key app, show it there, and bring its response back. | the owner |  |
| editQueuedTightening | Change signed | The change is signed by this phone and waits to be delivered to the counters. | Nothing more is needed. See it under Waiting to be delivered. | the owner |  |
| editNodeNeeded | Node key needed | The node's key or address was left empty, so no node was named. | Paste or scan the key or address of the node, or choose Remove the node. | the owner |  |
| editNoCounters | No counters yet | The settings list no counters, so there are no shares to change. | Add a counter first. Equal shares are made when it is added. | the owner |  |
| editSharesNotPercent | Share not accepted | A share must be a percent from 0 to 100, with up to two decimals, so nothing was saved. | Type each share again, like 50 or 33.33. | the owner |  |
| editValueNotReadDollars | Value not accepted | The value for Refunds per day is not a whole number of dollars, so nothing was saved. | Type whole dollars, like 25, or 0 to turn it off. | the owner |  |
| editValueNotReadPercent | Value not accepted | The value for Underpayment still counted is not a percent, so nothing was saved. | Type a percent with up to two decimals, like 0.5. | the owner |  |
| editValueNotReadBlocks | Value not accepted | The value for Blocks to settle a sale is not a whole number of blocks, so nothing was saved. | Type a whole number of blocks, like 6. | the owner |  |
| editValueNotReadMinutes | Value not accepted | The value for Heartbeat age for instant is not a whole number of minutes, so nothing was saved. | Type whole minutes, like 10. | the owner |  |
| editValueNotReadSats | Value not accepted | The value for Smallest bitcoin sale is not a whole number of satoshis, so nothing was saved. | Type whole satoshis, like 1000, or 0 for none. | the owner |  |
| editValueNotReadSwitch | Value not accepted | The value for Tor on the counters was not chosen, so nothing was saved. | Choose one of the two answers. | the owner |  |
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
| choosePin | Choose a PIN | Key 1 on this phone is kept behind a PIN, which the app asks for every time it opens. | Choose at least 6 digits, or a passphrase of at least 8 characters, and do not forget it. | the owner |  |
| upgradePin | Set a PIN for key 1 | Key 1 on this phone was saved without a PIN, so anyone who opens the app could use it. | Choose a PIN now; the setup is kept as it is and locked behind it. | the owner |  |
| unlockSetup | Enter your PIN | Key 1 on this phone is locked, so the app opens only with the PIN or passphrase. | Type it and tap Unlock. Too many wrong tries wipe key 1 from this phone. | the owner |  |
| pinsDiffer | The two PINs differ | The PIN typed the second time is not the same as the first, so nothing was set. | Type the same PIN in both fields, then try again. | the owner |  |
| pinChanged | PIN changed | Key 1 on this phone is now locked behind the new PIN. | Nothing to do. Use the new PIN from now on. | nobody |  |
| working | Working | The app is working with the PIN, which takes a moment. | Wait. Do not close the app. | nobody |  |
| waitForTry | Wait before the next try | There were too many wrong tries, so the app waits before it takes another. | Wait for the time shown, then type the PIN again. | the owner |  |
| triesNotSaved | Try not counted | The phone could not keep the count of wrong tries, so the PIN was not tried. | Try again. If it keeps failing, restart the phone and open this app again. | the owner |  |
| wiped | Key 1 wiped from this phone | This phone's copy of key 1 was wiped after 10 wrong tries. The money is not lost: two of the three keys still exist. | Follow the recovery kit to move to a new wallet, or ask the person who holds key 2. | the owner |  |
| wipeFailed | Wipe not finished | Key 1 could not be wiped from this phone completely, so the app will try again when it opens. | Close the app and open it again. If it keeps failing, uninstall the app. | the owner |  |
| cameraOff | Camera is off for this app | This app is not allowed to use the camera, so it cannot scan a code. | Allow the camera for this app in the phone's settings, or paste the text or choose a file instead. | the owner |  |
| cameraNotStarted | Camera did not start | The camera could not be opened, so nothing was scanned. | Close any other app that uses the camera and try again, or paste the text or choose a file instead. | the owner |  |
| editField_light_per_sale | Light check: one sale | The largest sale accepted as instant when only a light check has seen the payment. | Whole dollars. 0 turns it off. | the owner |  |
| editField_light_budget | Light check: all at once | The most that may wait for confirmation together across the shop, on a light check. | Whole dollars. 0 turns it off. | the owner |  |
| editField_node_per_sale | Own node: one sale | The largest sale accepted as instant when the shop's own node has also seen the payment. | Whole dollars. 0 turns it off. | the owner |  |
| editField_node_budget | Own node: all at once | The most that may wait for confirmation together across the shop, with the own node. | Whole dollars. 0 turns it off. | the owner |  |
| editField_box_per_sale | Box only: one sale | The largest sale accepted as instant when only a Box reports the payment. | Whole dollars. 0 turns it off. | the owner |  |
| editField_box_budget | Box only: all at once | The most that may wait for confirmation together across the shop, on a Box's word. | Whole dollars. 0 turns it off. | the owner |  |
| editField_underpayment_tolerance_bps | Underpayment still counted | How far under the price a payment may fall and still count as paid. | A percent with up to two decimals, from 0 to 5. | the owner |  |
| editField_min_confirmations | Blocks to settle a sale | How many blocks a payment needs before the sale counts as settled. | A whole number from 1 to 100. | the owner |  |
| editField_final_confirmations | Blocks until final | How many blocks until a sale is final and a refund may be paid. It is at least the number above. | A whole number from 1 to 100. | the owner |  |
| editField_min_sale_sats | Smallest bitcoin sale | A bitcoin sale below this is turned away, because a tiny receipt can cost more to spend than it holds. | Whole satoshis. 0 means no smallest sale. | the owner |  |
| editField_refund_cap | Largest single refund | The most one refund may pay back to a customer. | Whole dollars. 0 turns it off. | the owner |  |
| editField_refund_daily_cap | Refunds per day | The most that may be refunded in one day, all refunds together. | Whole dollars. 0 turns it off. | the owner |  |
| editField_cashier_override_daily_cap | Cashier's override per day | The most one cashier may accept against a red screen in a day. | Whole dollars. 0 turns it off. | the owner |  |
| editField_manager_override_ceiling | Manager's override ceiling | The most a manager may accept against a red screen, over all their overrides. | Whole dollars. 0 turns it off. | the owner |  |
| editField_heartbeat_max_age_secs | Heartbeat age for instant | How old this phone's last report may be before counters stop accepting instant sales. | Whole minutes, from 1 to 60. | the owner |  |
| editField_tor_on | Tor on the counters | Whether the counters connect through Tor. Turning it off removes that protection. | Choose On or Off. | the owner |  |
| editField_customer_wifi | Customers can join the Wi-Fi | Whether customers can join the Wi-Fi a counter is on, or you are not sure. No removes a Tor requirement. | Choose Yes or unsure, or No. | the owner |  |
| boxAddressNotRead | Address not read | The Box's address is host:port on your network, or a v3 .onion name and a port, so nothing was tried. | Type it again, for example 192.168.1.20:8080. | the owner |  |
| boxLooking | Looking for the Box | The phone is asking the Box for its claim, which can take up to a minute. | Wait on this screen. | the owner |  |
| boxPriceNeeded | Price for the Box | This phone holds no settings yet, and the starting limits are in dollars, so it needs today's price of a bitcoin. | Type today's price of one bitcoin in whole dollars, then queue the Box. | the owner |  |
| boxCompare | Compare the four words | The Box shows four words on its own screen. They must be the same as these, and the Key app will show them too. | If they differ, go back. Nothing is paired until the Key app approves. | the owner |  |
| boxQueued | Box queued | The Box's claim and the settings that add it wait for the Key app. | Build the request for the Key app, approve there, and bring its response back. | the owner |  |
| boxHanding | Handing over the welcome | The Box is being given its welcome and the settings, which can take up to a minute. | Wait on this screen. | the owner |  |
| boxHandoverKept | The approval is kept | The Key app's approval stays queued on this phone. Nothing was lost and the Box is not paired yet. | Try again here once the Box answers. If the invitation ran out, make a new one on the Key app. | the owner |  |
| boxPaired | Box paired | The Box took its welcome and this phone saved the link to it, with the counters the settings name. | The link starts now. Keep Orbot running if the Box is a .onion address. | the owner |  |
| boxLinkNotSaved | Link not saved | The Box took its welcome, but this phone could not save the link to it. | Add the lines below to the setup text, then start the app again. | the owner |  |
| boxSavePin | Save the link | The Box took its welcome. The link to it is saved in the locked setup, so the PIN is needed to open it and lock it again. | Type the PIN of this phone, then save the link. | the owner |  |
| boxForgotten | Box forgotten | The Box's claim and the settings that add it were taken out of the queue. The Box itself was not told. | To add it again, make a new invitation on the Key app and connect the Box again. | the owner |  |
| UrWords.reading | Reading the animated code | Reading the animated code: 3 of about 9 parts. | Keep the phone pointed at the code until it is done. | the owner |  |
| UrWords.restarted | Code did not add up | The parts read did not fit together, so reading starts again. | Keep the phone pointed at the code. It reads from the first part again. | the owner |  |
| UrWords.otherCode | Another code is in view | The code in view is not part of the one being read, so it is skipped. | Point the phone at the first code again, or cancel and start over. | the owner |  |
| UrWords.wrongKind | Not the code expected | This code carries something this step does not take, so nothing was used. | Cancel, then ask for the right code again, or use a file instead. | the owner |  |
| UrWords.contentRefused | Code content not usable | The code was read in full, but what it holds is not in a form this step takes. | Ask for the code to be shown again, or use a file instead. | the owner |  |
| UrWords.readerFailed | Code reader failed | The part of this app that reads codes gave no usable answer, so nothing was read. | Close the app and open it again. If it happens again, use a file. | the owner |  |
| hardwareChooseTransport | How to reach the signer | The payment goes to the signer as moving codes on this screen or as a file, and the signed payment comes back the same way. | Pick the one your signer uses. The names under each choice are examples. | the owner |  |
| hardwareSignerReview | Check it on the signer | The signer's own screen is the second check. It shows each address, each amount and the fee. | Compare every address and amount with this screen. Approve on the signer only if they all match. | the owner |  |
| hardwareRegisterIntro | Register the wallet | A signer signs for a wallet only after it has been shown the wallet. | Show the signer this code or file, then check the first address. | the owner |  |
| hardwarePartsMissing | Keep scanning | Some parts of the moving code have not been read yet. They can be read in any order. | Hold the camera on the signer's screen until every part is read. | the owner |  |
| hardwareFirstAddressCompare | Check the first address | On the signer, open its address list and find the first receive address. | Compare it with the address shown here, character by character. | the owner |  |
| hardwareFirstAddressDiffers | Do not use with this signer | The signer shows a different first address, so it holds a different wallet or was registered wrongly. | Register the wallet again on the signer, then check again. If it still differs, do not take money into this wallet. | the owner |  |
| hardwareFirstAddressChecked | First address checked | The signer shows the same first address as this phone, so it holds the same wallet. | Nothing to do. | nobody |  |
| hardwareFirstAddressSkipped | First address not checked | Nothing yet shows that the signer holds the same wallet as this phone. | Check it from the wallet screen before the shop takes money. | the owner |  |
| hardwareReturnedNeeded | Bring the payment back | The signer has the payment. Its signature has to come back to this phone before the payment can be sent. | Scan the code the signer shows, or open the file it saved. A payment the signer finished itself works too. | the owner |  |
| hardwareFileSaved | File saved | The payment for the signer is in the file named below. | Put the file on the signer's memory card and sign it there. Then bring the signed file back. | the owner |  |
