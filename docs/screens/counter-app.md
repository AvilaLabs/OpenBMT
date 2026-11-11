# Counter app: what the app itself tells a person

Made by `AppWords.screensMarkdown` in
`apps/counter-android/src/model/AppWords.kt`. Do not edit it by hand: run
`apps/counter-android/tests/run-jvm.sh` with the environment variable
`OPENBMT_WRITE_SCREENS=1` to write it again.

Everything about a sale, the health of the check, the price and the
refusals of the rules is shown with the words of the library
(`docs/screens/counter.md`, `docs/screens/rates.md`, `openbmt-counter-ffi`).
These are the words of what the app decides itself.

| State | Heading | What happened | What to do next | Who | Code |
|---|---|---|---|---|---|
| notSetUp | Not set up yet | This counter has not been paired with the shop yet, so it cannot take a sale. | Type a name for the counter, the manager's name and a PIN. Then tap Pair with the Key app and scan its code with this phone. | a manager |  |
| pairingSaved | Counter set up | The counter is paired with the shop, and the pairing and the PIN are saved on this phone. | Keep the app open at the counter. It starts looking for the network now. | the cashier |  |
| pairingScanOffer | Scan the Key app | The Key app shows a code on its Pair a counter screen. This counter reads the shop's keys and network from it. | Open Pair a counter on the Key app, then tap Scan and hold this phone's camera to its code. | a manager |  |
| pairingCompare | Compare the four words | The Key app shows four words for this pairing, and the same four are below. If they are not the same, someone changed the code on the way. | Check all four words on the Key app, then tap They match, or tap They differ if any word is not the same. | a manager |  |
| pairingScanBundle | Scan the owner's phone | The four words matched. The owner's phone now shows the code that finishes the pairing. | On the owner's phone open the counter's code, then tap Scan and hold this phone's camera to it. | a manager |  |
| pairingForgotten | Pairing not finished | The app was closed or left before the pairing was finished, so it was forgotten and nothing was saved. | Start again from the Key app: open Pair a counter, then tap Pair on this counter and scan its code. | a manager |  |
| counterNameNotAccepted | Counter name not accepted | The name of this counter is 1 to 32 characters long, so it was not used. | Type a shorter name for this counter, then tap Pair with the Key app again. | a manager |  |
| pinNotAccepted | PIN not accepted | A PIN is 6 to 12 digits, and the two entries must be the same, so nothing was saved. | Type a PIN of 6 to 12 digits twice, then save again. | a manager |  |
| nameNotAccepted | Name not accepted | A name is 1 to 64 characters long, so it was not saved. | Type the name again. | the cashier |  |
| torAddressNotAccepted | Tor address not accepted | The address of Orbot must be a host, a colon and a port, like 127.0.0.1:9050. | Type the address again. | a manager |  |
| torIsOn | Tor is on for this shop | The shop's settings send every connection of this counter through Tor, with Orbot. | Keep Orbot running. The manager can check its address under Network. | a manager |  |
| torIsOff | Tor is off for this shop | The shop's settings do not use Tor, so the price sources and the network can see this shop's address. | Nothing to do here. The owner can switch Tor on in the shop's settings. | the owner |  |
| orbotNotRunning | Tor is not running | Tor is on for this shop, but Orbot is not running. | Open Orbot and turn it on. If it listens at another address, the manager can change it under Network. | a manager |  |
| networkAbout | Network | When the shop's settings turn Tor on, this counter sends everything through Orbot, at the address below. | Change the address only if Orbot listens somewhere else, then type the manager's PIN. | a manager |  |
| networkSaved | Address saved | The counter will look for Orbot at the new address. | Tap Start to use it now. | a manager |  |
| ownNodeAbout | Your own node | A node of your own, reached by a Tor address, can check each payment as well as the light check does. | Type its Tor address and the manager's PIN, then save. | a manager |  |
| ownNodeSaved | Own node saved | The counter will ask your own node about each payment, once it has started again. | Wait a moment while the counter starts again. | a manager |  |
| ownNodeRemoved | Own node removed | The counter checks payments with the light check only. | Wait a moment while the counter starts again. | a manager |  |
| lightningAbout | Lightning through your node | The counter only asks your node for invoices, so give it a key limited to invoices, from a node that runs a current release. | Scan or paste the LND code, or fill in the three Core Lightning fields, type the manager's PIN, then test and save. | a manager |  |
| lightningCare | Running a Lightning node | A node that takes payments needs channels with room to receive, updates kept current and a backup of its own. | Watch the channel balances, install each update soon after it comes out, and keep a backup of the node. | the owner |  |
| lightningTesting | Testing the node | The counter is asking your node for a test invoice and checking that it is yours. | Wait a moment while the node answers. | a manager |  |
| lightningFieldsNeeded | Lightning fields needed | The connection needs its code, or for Core Lightning its address, its key and its certificate. | Fill them in, then test and save again. | a manager |  |
| lightningNotReached | Test did not finish | Your node gave no answer the counter could use, so nothing was saved. | Check the address and that the node is running, then try again. | a manager |  |
| lightningSaved | Lightning saved | The test passed, and the connection is saved on this phone where only this app can read it. | Wait a moment while the counter starts again. | a manager |  |
| lightningRemoved | Lightning removed | The connection to your node is deleted from this phone, and sales show the on-chain code only. | Nothing more to do. | a manager |  |
| lightningWaiting | Lightning code coming | The counter is asking the shop's node for a Lightning code for this sale. | Let the customer pay on-chain with the code above, or wait a few seconds. | the cashier |  |
| managerNeeded | A manager is needed | This step needs the manager's PIN. | Ask the manager to type the PIN. | a manager |  |
| pinWrong | Wrong PIN | That PIN is not right, so nothing was done. | Wait 4 seconds, then try again. | a manager |  |
| pinLocked | PIN locked for now | There were wrong tries in a row, so the PIN is locked for a while. | Wait 5 minutes, then try again. | a manager |  |
| staffPinNeeded | Manager's PIN needed | The list of cashiers is changed only with the manager's PIN. | Type the manager's PIN, then tap Open. | a manager |  |
| staffAbout | Staff | Cashiers sign in at the till with their name and PIN. Each one added or removed is written down for the owner. | Add a cashier below, or remove one, or give one a new PIN. | a manager |  |
| cashierNameNotAccepted | Cashier name not accepted | A cashier's name is 1 to 24 characters long, with no space at either end, so it was not used. | Type another name, then add the cashier again. | a manager |  |
| cashierNameUsed | Name already used | Another cashier or the manager has that name, whatever the capital letters, so it was not used. | Type a different name, then add the cashier again. | a manager |  |
| cashiersFull | Cashier list is full | The list holds at most 30 cashiers, so no one more can be added. | Remove a cashier first, then add the new one. | a manager |  |
| cashierPinNotAccepted | Cashier PIN not accepted | A cashier's PIN is 4 to 8 digits, and the two entries must be the same, so nothing was saved. | Type the PIN twice, then save again. | the cashier |  |
| cashierAdded | Cashier added | The cashier is on the list, and the owner's phone gets a record of it. | Sign in at the till with the name and the PIN. | the cashier |  |
| cashierRemoved | Cashier removed | The cashier is off the list and can no longer sign in, and the owner's phone gets a record of it. | Nothing more to do. | nobody |  |
| cashierPinSaved | New PIN saved | The cashier's new PIN is saved on this phone. Nothing about it is sent to the owner. | Sign in at the till with the new PIN. | the cashier |  |
| ownerNotTold | Owner not told | The list of cashiers was changed, but the record for the owner could not be written. | Tell the owner in person that the list changed. | a manager |  |
| cashiersNotRead | Cashier list not read | The list of cashiers could not be read, so nobody can sign in and nothing was done. | Try again. If it happens again, stop and tell the owner in person. | a manager |  |
| whoAtTill | Who is at the till? | The till is locked, so no sale can be opened until a person signs in. | Tap your name, then type your PIN. | the cashier |  |
| signInWrong | PIN not right | That PIN is not the one for this name, so nobody was signed in. | Wait 4 seconds, then try again. | the cashier |  |
| signInHeld | PIN held for now | There were wrong tries in a row on this name, so its PIN is held for a while. | Wait 5 minutes, then try again, or tap Back and pick another name. | the cashier |  |
| overrideAbout | Accept by hand | Accepting a sale by hand is written down with the name of the person who does it, and the limits apply to each person. | Tap Accept, or choose the manager and type the manager's PIN. | the cashier |  |
| todayAbout | Today at this counter | These totals are counted from this counter's own records since midnight. No wallet balance is shown here. | The owner's phone has the full books and the export. | the owner |  |
| todayWaiting | Why sales are waiting | A sale waits until its payment is accepted: it may be unpaid, part paid, or paid and not yet confirmed. | Open the sale from Sales to see which. The count changes once the payment is accepted or the sale ends. | the cashier |  |
| todayStopped | Sales not to be handed over | A payment for these sales was replaced or reopened, so the goods are not to be handed over. | Open the sale from Sales and follow what it says. | a manager |  |
| todayPeopleNeedPin | Lines per person | The lines per person show who opened each sale and did each override or refund, so they need the manager's PIN. | Type the manager's PIN, then tap Show the lines. | a manager |  |
| priceReady | Price ready | Bitcoin is priced at 73,554.18 USD, from 3 price sources. | Nothing to do. The app asks the sources again every minute. | nobody |  |
| sourceNotAnswered | Price source not reached | A price source did not answer in time, so its price is missing this turn. | Nothing to do. The app asks again in a minute. | nobody |  |
| sourceRefusedStatus | Price source said no | A price source answered with a refusal, so its price is missing this turn. | Nothing to do. The app asks again in a minute. | nobody |  |
| sourceReplyTooLong | Price reply too long | A price source sent more than the app reads, so its price is missing this turn. | Nothing to do. The app asks again in a minute. | nobody |  |
| amountNeeded | Amount needed | Type an amount above zero to start a sale. | Press the digits of the amount, then press Open. | the cashier |  |
| cameraOff | Camera is off for this app | This app is not allowed to use the camera, and pairing needs it to read the codes, so nothing was paired. | Allow the camera for this app in the phone's settings, then start the pairing again. | a manager |  |
| cameraNotStarted | Camera did not start | The camera could not be opened, so no code was read and nothing was paired. | Close any other app that uses the camera, then start the pairing again. | a manager |  |
| notANumber | Check what you typed | The amount is empty or has something other than digits in it. | Type whole numbers only, then try again. | the cashier |  |
| nameNeeded | Name needed | A refund or an override is written down with the name of the person who asked for it. | Type your name on the home screen, then try again. | the cashier |  |
| destinationNeeded | Address needed | A refund needs the address the customer wants the money sent to. | Paste the address or the payment request the customer shows, then try again. | the cashier |  |
| reasonNeeded | Reason needed | A refund is written down with the reason for it. | Type a short reason, then try again. | the cashier |  |
| requestCopied | Request copied | The payment request is copied, written in lower case. | Paste it where the customer can use it. | the cashier |  |
| refundAsked | Refund asked for | The refund request is saved, and it is paid only after the owner approves it. | Nothing more to do here. Tell the customer the owner pays refunds from the owner's phone. | the cashier |  |
| refundWithdrawn | Refund withdrawn | The request is withdrawn, and the owner is told it is not wanted. | Nothing to do. If the owner has already paid it, tell the owner. | nobody |  |
| cashRefunded | Cash refund saved | The cash refund from the till is written down, and the bitcoin is kept. | Give the customer the cash. | the cashier |  |
| starting | Starting | The counter is starting and looking for the network. | Wait a moment. Keep the app open. | nobody |  |
| stopped | Counter stopped | The counter has stopped, so no sale can be opened. | Close the app and open it again. If it stays stopped, take another kind of payment. | the cashier |  |
| libraryMissing | The app is incomplete | The part of this app that runs the counter did not load, so nothing was done. | Do not use this copy. Install the app again from the place you got it. | the owner |  |
| fault | Answer not readable | The app could not read an answer from the counter part, so nothing was changed. | Close the app and open it again. If it happens again, stop and tell the owner in person. | the owner |  |
| storageUnavailable | Phone storage not usable | The phone's protected storage did not start, so the saved pairing was not opened. | Restart the phone and open this app again. If it happens again, stop and tell the owner in person. | the owner |  |
| fileNotRead | File not read | The file could not be opened or read, so nothing was done. | Try again. If it happens again, stop and tell the owner in person. | a manager |  |
| fileNotSaved | Not saved | The app could not write to this phone's storage, so nothing was saved. | Try again. If it happens again, stop and tell the owner in person. | a manager |  |
| UrWords.reading | Reading the animated code | Reading the animated code: 3 of about 9 parts. | Keep the phone pointed at the code until it is done. | a manager |  |
| UrWords.restarted | Code did not add up | The parts read did not fit together, so reading starts again. | Keep the phone pointed at the code. It reads from the first part again. | a manager |  |
| UrWords.otherCode | Another code is in view | The code in view is not part of the one being read, so it is skipped. | Point the phone at the first code again, or cancel and start over. | a manager |  |
| UrWords.wrongKind | Not the code expected | This code carries something this step does not take, so nothing was used. | Cancel, then ask for the right code again, or use a file instead. | a manager |  |
| UrWords.contentRefused | Code content not usable | The code was read in full, but what it holds is not in a form this step takes. | Ask for the code to be shown again, or use a file instead. | a manager |  |
| UrWords.readerFailed | Code reader failed | The part of this app that reads codes gave no usable answer, so nothing was read. | Close the app and open it again. If it happens again, use a file. | a manager |  |
