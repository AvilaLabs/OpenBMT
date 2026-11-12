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
| setupSaved | Owner phone set up | The setup is saved on this phone, sealed with a key that stays in the phone. | Keep the app open. It starts looking for the network now. | the owner |  |
| setupNotReadable | Setup text not read | The setup text is not in the form this app reads, so nothing was saved. | Copy the setup text again from the key ceremony, then try again. | the owner |  |
| torAddressNotAccepted | Tor address not accepted | The address of Orbot must be a host, a colon and a port, like 127.0.0.1:9050. | Type the address again. | the owner |  |
| torAlwaysOn | Everything goes through Tor | This app sends every connection through Tor, with Orbot, and has no way to turn that off. | Check the address of Orbot below, and keep Orbot running. | the owner |  |
| orbotNotRunning | Tor is not running | This app sends everything through Tor, but Orbot is not running. | Open Orbot and turn it on. | the owner |  |
| torBuiltIn | Tor | This app runs Tor itself, so Orbot is not needed. | Nothing to do. To use Orbot instead, choose Use Orbot. | nobody |  |
| torUsesOrbot | Tor through Orbot | This app sends everything through Orbot, which must be installed and running. | Keep Orbot running. To stop needing it, choose Built-in Tor. | the owner |  |
| torStarting | Starting Tor | This app is connecting to the Tor network, which can take up to a minute. | Wait on this screen. | nobody |  |
| torFailed | Tor did not start | This app could not connect to the Tor network. | Check that the phone is online, then try again. If it keeps failing, choose Use Orbot. | the owner |  |
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
| counterPriceFetching | Getting today's price | This phone is asking the price sources for the price of one bitcoin, through Tor. | Wait a moment. If they do not answer, you can type the price. | nobody |  |
| counterPriceFetched | Today's price | The price sources give 98,500 dollars for one bitcoin. The starting limits are worked out from it. | If it looks right, say the words match. To use another price, type it below. | the owner |  |
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
| watchSetting | Watch in the background | The phone keeps following the shop's wallet while the app is closed, and tells the counters it is watching. | Turn it on to keep instant sales working. Android shows a notification while it runs. | the owner |  |
| watchNotificationsOff | Notifications are off | Android needs to show a notification while the phone watches in the background, and notifications are off for this app. | Allow notifications for OpenBMT Owner in Android's settings, then turn this on again. | the owner |  |
| requestSaved | Request saved | The request is saved as a file. | Carry the file to the Key app and open it there. | the owner |  |
| refundQueuedAtDepth | Refund queued | The refund is in the queue, and the sale is deep enough to be refunded. | Bring this phone and the Key app together and start an approval. | the owner |  |
| refundQueuedWaiting | Refund queued, sale new | The refund is in the queue, but the sale is not deep enough yet to be refunded. | Wait for more blocks. The refund can be approved when the sale is deep enough. | the owner |  |
| cashOutQueued | Cash-out queued | The cash-out is in the queue and has not been sent for approval yet. | Bring this phone and the Key app together and start an approval. | the owner |  |
| cashOutPrice | Today's price | The price sources give 98,500 dollars for one bitcoin. The books record this cash-out at that price. | If it looks right, go on. To use another price, type it below. | the owner |  |
| cashOutPriceNeeded | Price needed | The price sources did not answer, and the books need what this cash-out is worth. | Type the price of one bitcoin, then go on. | the owner |  |
| booksEmpty | No sales yet | No counter has sent a sale to this phone yet. | Nothing to do. Sales appear here as the counters report them. | nobody |  |
| booksSaved | File saved | The file is saved where you chose. It holds sale numbers, times and amounts, and no addresses. | Give it to your bookkeeper. | the owner |  |
| entryCancelled | Entry cancelled | The entry is taken out of the queue. | Nothing to do. | nobody |  |
| entryRequeued | Entry queued again | The entry that was sent back is in the queue again. | Start an approval when the Key app is at hand. | the owner |  |
| sessionAbandoned | Approval abandoned | The open approval is closed, and its entries are back in the queue. | Nothing to do. Start a new approval when the Key app is at hand. | nobody |  |
| abandonExported | Already sent to the signer | This approval went to the signer, so it may already be signed and could still be sent. | If you abandon it, the payments go back in the queue and a new approval spends the same coins, so only one can ever be paid. | the owner |  |
| abandonedExported | Back in the queue | The payments are back in the queue. The payment sent to the signer is still watched: if it is sent, it counts as paid. | Start a new approval when you are ready. | the owner |  |
| alertCleared | Alert cleared | The alert is cleared from this phone. | Nothing to do. If the cause is still there, the alert comes back. | nobody |  |
| noteDismissed | Note dismissed | The note about a refund request is removed from the list. | Nothing to do. | nobody |  |
| noLink | No link to the counters | This phone was set up without a relay, so no counter can report to it. | Nothing to do here. A setup text with a relay adds the link. | the owner |  |
| noSettingsYet | No settings yet | The phone holds no settings for the shop yet, so there is nothing to show. | Nothing to do. They arrive with the first approval that carries settings. | the owner |  |
| editIntro | Change the settings | Pick a group, change what you need, then review. Nothing is queued until you press Queue the change. | A change that loosens something needs the Key app. A stricter change does not. | the owner |  |
| editGroupCurrency | Currency | The money the counters price sales in, and the limits are set in. | Choose one. Bitcoin Standard prices in sats and uses no price source. | the owner |  |
| editCurrencyRetype | New currency, new limits | Every limit is set again in the new currency, so the change shows them empty. | Type each limit in the new currency, then review. The change needs the Key app. | the owner |  |
| editGroupInstant | Instant sales | How much a counter may accept as paid before the payment is confirmed, for each way it checks. | Whole dollars. 0 turns a limit off. | the owner |  |
| editGroupPayments | Payments | When a payment counts, how many blocks settle a sale, and the smallest bitcoin sale. | Change what you need, then press Done. | the owner |  |
| editGroupTips | Tips | Whether the counters ask the customer for a tip, and which tips they suggest. | Turn tips on, and set up to three percents. | the owner |  |
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
| editTipNotRead | Tip not accepted | A suggested tip must be a whole percent from 1 to 50, so nothing was saved. | Type it again, like 15, or leave it empty. | the owner |  |
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
| posNotReached | Till not reached | The till's system did not answer through Tor, so nothing was read. | Check that the phone is online and Tor is connected. The next read is in about a minute. | the owner |  |
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
| editField_tips_on | Ask for a tip | Whether the counters ask the customer for a tip, and which tips they suggest. | Turn tips on, and set up to three percents. | the owner |  |
| editField_tip_1 | First tip | Whether the counters ask the customer for a tip, and which tips they suggest. | A percent from 1 to 50, or empty. | the owner |  |
| editField_tip_2 | Second tip | Whether the counters ask the customer for a tip, and which tips they suggest. | A percent from 1 to 50, or empty. | the owner |  |
| editField_tip_3 | Third tip | Whether the counters ask the customer for a tip, and which tips they suggest. | A percent from 1 to 50, or empty. | the owner |  |
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
| boxPriceFetching | Getting today's price | This phone is asking the price sources for the price of one bitcoin, through Tor. | Wait a moment. If they do not answer, you can type the price. | nobody |  |
| boxPriceFetched | Today's price | The price sources give 98,500 dollars for one bitcoin. The starting limits are worked out from it. | If it looks right, say the words match. To use another price, type it below. | the owner |  |
| boxCompare | Compare the four words | The Box shows four words on its own screen. They must be the same as these, and the Key app will show them too. | If they differ, go back. Nothing is paired until the Key app approves. | the owner |  |
| boxQueued | Box queued | The Box's claim and the settings that add it wait for the Key app. | Build the request for the Key app, approve there, and bring its response back. | the owner |  |
| boxHanding | Handing over the welcome | The Box is being given its welcome and the settings, which can take up to a minute. | Wait on this screen. | the owner |  |
| boxHandoverKept | The approval is kept | The Key app's approval stays queued on this phone. Nothing was lost and the Box is not paired yet. | Try again here once the Box answers. If the invitation ran out, make a new one on the Key app. | the owner |  |
| boxPaired | Box paired | The Box took its welcome and this phone saved the link to it, with the counters the settings name. | The link starts now. | the owner |  |
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

What Android shows for the background watch, which is not a screen of the app:

- Rest of the setting's text:
  Without this, instant sales stop about ten minutes after you close the app.
- Notification title:
  Watching the shop
- Notification text:
  Instant sales stay on while this runs.
- Notification action:
  Stop
- Notification when Tor is not connected:
  Not watching: Tor is not connected. Instant sales are off.

The first run: the welcome, the practice shop, the shop, the till and its
steps, in `FirstRunWords`. `{till}` is the till's name as listed:

| Key | Text |
|---|---|
| welcomeTitle | Take bitcoin in your shop. |
| welcomeText | It goes straight to your own wallet. Nobody holds it for you, and nobody takes a cut of your sales. |
| welcomeTry | Try it first |
| welcomeSetup | Set up my shop |
| welcomeTryNote | A practice shop on this phone. No money, no wallet, nothing to buy. |
| practiceBanner | PRACTICE. No real money. |
| practiceIntroTitle | Here is a sale, start to finish |
| practiceIntroText | You ring it up, the customer scans the code with their bitcoin wallet, and the sale shows Paid. In your shop this runs on the counter, the phone or tablet at the till. |
| practiceStart | Ring up a sale |
| practiceAmountTitle | Amount |
| practiceCharge | Charge |
| practiceQrTitle | The customer scans this |
| practiceQrText | In a real sale this code asks for the exact amount in bitcoin, at today's price. This one is not real and cannot be paid. |
| practicePayButton | Pretend the customer paid |
| practicePaidTitle | Paid |
| practicePaidText | The bitcoin went to your wallet. Hand over the goods. |
| practiceBooksTitle | Your books |
| practiceBooksText | Every sale is recorded with its amount in your money and in bitcoin, ready for your bookkeeper. |
| practiceRefundButton | Try a refund |
| practiceRefundText | A refund is given in cash from the till straight away, or in bitcoin, which you approve later from your own wallet. |
| practiceRefundDone | Refunded in cash |
| practiceDoneTitle | That is the whole sale |
| practiceDoneText | Setting up your shop takes about ten minutes. Making a wallet, if you need one, takes about an hour and can wait. |
| practiceSetup | Set up my shop |
| practiceAgain | Try again |
| practiceDelete | Delete the practice shop |
| practiceDeleted | Practice shop deleted. |
| shopTitle | Your shop |
| shopNameLabel | Shop name |
| shopNameHint | As customers know it |
| shopCurrencyLabel | Your money |
| shopCurrencyNote | Prices are set in this money and shown in bitcoin at the till. |
| moneyTabLocal | Local money |
| moneyTabBitcoin | Bitcoin Standard |
| moneyBitcoinTitle | Prices in sats |
| moneyBitcoinNote | Prices are set in sats, and the books count in sats. No exchange rate is used. |
| moneyTabsSpoken | Your money: {tab} chosen. Double tap the other tab to switch. |
| shopNext | Next |
| tillTitle | How do you take payments now? |
| tillText | Bitcoin is added to your till as one more way to pay. Nothing else about your till changes. |
| tillSearch | Search tills |
| tillNone | No till. I write sales down or use a cash box. |
| tillOther | Another till |
| tillNext | Next |
| tillStepsTitle | Add Bitcoin to {till} |
| tillStepsIntro | Do this once, on the computer or tablet where you manage {till}. It takes about two minutes. |
| tillStepsCashier | At the till, the cashier rings up the sale in {till} as usual, takes the bitcoin on the counter, then picks Bitcoin as the payment in {till}. |
| tillStepsNoFee | {till} does not move the money, so it charges no fee for this payment type. |
| tillStepsChecked | Steps read on {till}'s own help pages on 11 November 2026. Menus change; if yours looks different, look for "payment types". |
| tillStepsDone | Done |
| tillSquare | Square |
| tillSquareStep1 | In Square Dashboard, open Settings, then Payments, then Payment methods. |
| tillSquareStep2 | Open Custom payment methods and tap Add payment method. |
| tillSquareStep3 | Name it Bitcoin. Square may not let you rename it later, so check the spelling. |
| tillSquareStep4 | Turn on Activate on Point of Sale, then tap Add. |
| tillSquareCashier | Cashier: Charge, then More, then Custom payment methods, then Bitcoin. |
| tillSquareReports | Reports: bitcoin sales are under Other in the sales summary. |
| tillToast | Toast |
| tillToastStep1 | In Toast Web, open the payment settings and find Other payment options. |
| tillToastStep2 | Tap Add and name it Bitcoin. |
| tillToastStep3 | Set Show on POS to Yes, then Save and publish. |
| tillToastCashier | Cashier: Pay, then Other payments, then Bitcoin. |
| tillShopify | Shopify POS |
| tillShopifyStep1 | In the Shopify POS app, open Settings, then Payment settings. |
| tillShopifyStep2 | Open Custom payment types and tap Add custom payment type. |
| tillShopifyStep3 | Name it Bitcoin, save, and turn it on. |
| tillShopifyNote | It is on for every POS device unless you turn it off on one. |
| tillLoyverse | Loyverse |
| tillLoyverseStep1 | In Loyverse Back Office, open Settings, then Payment types. |
| tillLoyverseStep2 | Tap Add payment type, pick Other, and name it Bitcoin. |
| tillLoyverseStep3 | Choose the stores that take it, then Save. |
| tillLoyverseReports | Reports: Sales by payment type shows bitcoin on its own line. |
| tillOdoo | Odoo |
| tillOdooStep1 | In Odoo, open Point of Sale, then Configuration, then Payment Methods. |
| tillOdooStep2 | Tap New and name it Bitcoin. |
| tillOdooStep3 | Pick a journal of its own for it, leave Integration as None, and choose the points of sale that take it. Save. |
| tillHike | Hike |
| tillHikeStep1 | In Hike, open Setup, then Payment Types. |
| tillHikeStep2 | Tap Add Payment Type, name it Bitcoin, choose Custom, and save. |
| tillHikeStep3 | Under Register Availability, choose the registers that take it. |
| tillStoreHub | StoreHub |
| tillStoreHubStep1 | In StoreHub BackOffice, open Settings, then Payment Options. |
| tillStoreHubStep2 | Tap Add Payment Option, name it Bitcoin, and choose Custom. |
| tillStoreHubStep3 | Choose the stores and registers, then Save. |
| tillStoreHubStep4 | On each register, open the menu and tap Sync, twice. |
| tillStoreHubCashier | Cashier: if Bitcoin is not shown at charge, tap Others. |
| tillNoneText | The counter keeps every sale with its time, amount and who took it. Your books come from there. |
| tillOtherText | Most tills let you add your own payment type. In your till's settings, look for "payment types" or "payment methods", add one called Bitcoin, and turn it on for your registers. |
| tillOtherAsk | If your till has no such setting, the counter still keeps every sale for your books. |
| moneyTitle | Where should the money go? |
| moneyText | Bitcoin from your sales goes straight to a wallet you hold. OpenBMT never holds it and never makes your keys. |
| moneyOwn | Use my own wallet |
| moneyOwnNote | You already have a wallet and want sales to go there. |
| moneyMake | Help me make a cold wallet |
| moneyMakeNote | With a coin or a die and your laptop, offline. About an hour. |
| moneyLater | Set up the rest now, add the wallet later |
| moneyLaterNote | The counter says "Not taking bitcoin yet" until you do. |
| moneyNotYet | This part is still being built. For now, setup continues with the test setup. |
| ownAdviceTitle | We recommend a cold wallet |
| ownAdviceText | A cold wallet keeps its keys on a device that never connects to the internet, such as a hardware signer or an offline computer. Its 12 words are best made from your own coin flips or die rolls. |
| ownAdviceHot | A hot wallet, on a phone or an online computer, is fine for small amounts. Its keys sit on a device that can be hacked, so keep savings elsewhere. |
| ownAdviceMake | Make a cold wallet with a coin or a die |
| ownAdviceGo | Continue with my wallet |
| whyTitle | Why we do it this way |
| whyText1 | Any app can be hacked, ours included. So your keys never touch one. |
| whyText2 | You make them yourself, offline, with a coin or a die. After that, nobody can move your bitcoin without your words. Not even us. |
| whyText3 | The inconvenience happens once. |
| whyRealCases | Real cases |
| realTitle | When someone else holds the money |
| realBar | Real cases |
| realCount | {n} of {total} |
| realNext | Next case |
| realToBitcoin | Your own bitcoin |
| realDone | Done |
| realCase1Head | Six years on, most of their savings are still locked in the banks. |
| realCase2Head | Of €400,000 taken from one business, about €24,000 came back. |
| realCase3Head | 48,000 sellers have still not been paid for their own sales. |
| realCase4Head | $64.9 million in savings. $11.8 million offered back. |
| realIntro | Money in a bank or a payment app is not in your hands. Someone else decides when you can have it. In each case below, people could not get their own money out, through no fault of their own. |
| realCase1Place | Lebanon, since 2019 |
| realCase1Text | In 2019 Lebanon's banks stopped letting people take out their own dollars. Someone with $100,000 saved could get about $500 a month, paid in local money worth a third as much or less. Six years later most of it is still locked in the banks, and no law says they will get it back. |
| realCase1Quote | “I got to a point where I was going to sell my kidney…” |
| realCase1Who | A woman in Beirut, for her sister's cancer treatment |
| realCase2Place | Laiki Bank, Cyprus, 2013 |
| realCase2Text | Overnight, the government shut Laiki Bank and took every balance above €100,000 to cover the bank's losses. Nobody asked the account holders. A business with €500,000 there kept €100,000. Of the €400,000 taken, about €24,000 came back in six years. |
| realCase2Quote | “We can't make any withdrawals, we can't make deposits, pay our suppliers.” |
| realCase2Who | A florist in Nicosia, with the banks shut |
| realCase3Place | TMON and WeMakePrice, South Korea, 2024 |
| realCase3Text | Two large shopping websites held their sellers' money from every sale, then ran out of cash and stopped paying. About 48,000 sellers were cut off from $900 million they had already earned, about $19,000 each. The government's answer was to offer them loans: credit against their own unpaid money. |
| realCase3Quote | “If the settlement is not done by then, it means bankruptcy.” |
| realCase3Who | A seller of household goods in Seoul |
| realCase4Place | Synapse, United States, 2024 |
| realCase4Text | Synapse moved money between finance apps and the banks that held it. When it went bankrupt, accounts were frozen for months. One app's customers had $64.9 million in their accounts and were offered $11.8 million back. Deposit insurance did not cover them, because Synapse was not a bank. |
| realCase4Quote | “[The bank] was only going to pay us $500 out of that $280,000.” |
| realCase4Who | A former teacher who had saved for 15 years for a family home |
| realBitcoinHead | Your own bitcoin is different |
| realBitcoinText | Bitcoin in your own wallet is not held by a bank or a payment company. Nobody can put a hold on it, close the account or cap your withdrawals, because there is no account. It moves only when your keys sign. |
| realKeysTitle | Why the keys need so much care |
| realKeysText | With nobody in the middle, nobody can undo a mistake either. Whoever has your 12 words can spend the money, and if every copy is lost, nobody can give it back. So the words are made where no one can see them, written by hand, and put on steel. |
| realCoinTitle | Why a coin or a die |
| realCoinText | Phones and computers make random numbers with chips and software, and those have had faults before. A coin or a die has no chip and no software, so nothing can reach into it from outside. The key maker only turns your flips into words, offline, and shows its work so you can check it by hand. |
| realMore | More cases, with their sources and how each ended, are in the handbook under "When someone else holds the money". |
| kgTitle | Make your cold wallet |
| kgStepBadge | Step {n} of {total} |
| kgNext | Next |
| kgBack | Back |
| kgNeedTitle | What you need |
| kgNeedList1 | A laptop or desktop computer. It does not have to be new. |
| kgNeedList2 | Two USB sticks: one of 8 GB or more for Tails, and any small one for the key maker page. |
| kgNeedList3 | A coin or one six-sided die, a pen, and the blank card from this app. |
| kgNeedList4 | About an hour, somewhere nobody can see your screen. |
| kgNeedCard | No card? Any paper will do for now. Write "TEMPORARY. PUT THIS ON STEEL." at the top. |
| kgTailsTitle | Put Tails on the big stick |
| kgTailsText | Tails is a free system that starts the computer from the USB stick, never touches its own disk, and forgets everything when you shut down. On any computer, go to tails.net and follow its install guide for a USB stick. Check the download the way the guide says. |
| kgTailsNote | This step needs the internet. The rest does not. |
| kgPageTitle | Put the key maker page on the small stick |
| kgPageText | On the same computer, download the OpenBMT key maker page and its checksum file from the OpenBMT releases page, and copy the page onto the small stick. |
| kgPageCheck | To check it, compare its SHA-256 with the checksum file. The handbook shows how. |
| kgStartTitle | Start Tails, offline |
| kgStartText | Plug in the big stick and start the computer from it. On the Tails start screen, press + under Additional Settings, choose Offline Mode, and turn it on. Then Start Tails. |
| kgStartNote | With Offline Mode on, Tails turns off the network before anything else runs. |
| kgOpenTitle | Open the key maker page |
| kgOpenText | Plug in the small stick. Open it in the Files app and open openbmt-keymaker.html in Tor Browser. If it says the page did not start, reload it once. |
| kgFollowTitle | Follow the page |
| kgFollowText | Do the rehearsal first, then flip or roll for real. Come back to this phone when the page shows a code to scan. |
| kgScanButton | Scan the page's code |
| kgCompareTitle | Compare with the page |
| kgCompareText | The page and this phone must show the same fingerprint and first address, every character. |
| kgFingerprint | Fingerprint |
| kgFirstAddress | First address |
| kgCompareSame | They are the same |
| kgCompareDifferent | They are different |
| kgDifferentText | Stop. Do not use this wallet. On the page, press Clear and close, shut down Tails, and start again. |
| kgElectrumTitle | Check with Electrum |
| kgElectrumText | Now follow step 6 on the page. Electrum must show the same fingerprint and first address as this phone. |
| kgElectrumSame | Electrum shows the same |
| kgElectrumDifferent | Electrum shows something different |
| kgStopTitle | Stop |
| kgStartAgain | Start again |
| kgNotKeyMakerHeading | Not the key maker's code |
| kgNotKeyMakerWhat | This code is for a wallet with more than one key. The key maker makes a wallet with one key. |
| kgNotKeyMakerNext | Scan the code on the key maker page. |
| kgCameraOffHeading | The camera is off for OpenBMT |
| kgCameraOffWhat | The guide reads the page's code with the camera. |
| kgCameraOffNext | Allow the camera for OpenBMT in the phone's settings, then press Scan the page's code again. |
| kgDoneTitle | Shut down Tails |
| kgDoneText | On the page, press Clear and close, then shut down Tails and take out both sticks. Keep the card somewhere safe and private, and put the words on steel as soon as you can. |
| kgDoneButton | Done |

Connect your wallet, after the advice on an own wallet, in `ConnectWords`:

| Key | Text |
|---|---|
| introTitle | Connect your wallet |
| introText | Your wallet can show a public description of itself, called an output descriptor. OpenBMT uses it to make a new address for every sale. It cannot spend your money. |
| introWhere | Where to find it |
| introStep1 | Open your wallet on the computer or phone where it lives. |
| introStep2 | Find Export or Settings, then Output descriptor. |
| introStep3 | Show it as a code, then scan it with this phone. |
| introNote | Sparrow, Nunchuk and Bitcoin Core can show one. A wallet that shows only an xpub or zpub cannot be used yet. |
| introScan | Scan the code |
| introPaste | Paste the text |
| pasteHint | Paste the output descriptor here. |
| pasteCheck | Check |
| addressTitle | Check the first address |
| addressText | This is the first receive address of the wallet you scanned. In your wallet, open the list of addresses and find the first receive address. They must be the same. |
| addressKindSingle | Wallet with one key |
| addressKindMulti | Wallet with 2 of 3 keys |
| addressSame | Same address |
| addressDifferent | Different |
| differentTitle | Different wallet |
| differentText | The address is not your wallet's first address, so this is a different wallet, or the code was changed on the way. Nothing was saved. |
| differentNext | Scan the code again from the right wallet. |
| checkedTitle | Wallet checked |
| checkedText | This is the right wallet. Sales will go straight to it, and OpenBMT can see what arrives but can never spend it. |
| checkedNote | Connecting it to your counters is the next part, and it is still being built. |
| checkedNext | Next |

The key map at the top of the key-setup screens. `{k1}`, `{k2}` and `{k3}` are
the state words in lower case:

| Key | Text |
|---|---|
| keyMapKey1 | Key 1 |
| keyMapKey1Where | This phone |
| keyMapKey2 | Key 2 |
| keyMapKey2Where | Key app |
| keyMapKey3 | Key 3 |
| keyMapKey3Where | On steel |
| keyMapMade | Made |
| keyMapNext | Next |
| keyMapNotYet | Not yet |
| keyMapRule | Any 2 of the 3 keys can spend. One alone cannot. |
| keyMapSpoken | Key 1, on this phone: {k1}. Key 2, on the Key app: {k2}. Key 3, on steel: {k3}. Any 2 of the 3 keys can spend. |
| keyMadeTitle | Your keys |
| keyMadeHeading | Now make key 2 |
| keyMadeText | Open the Key app on the second phone and choose Set up keys. It shows a code. Scan that code with this phone. |
