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
| notSetUp | Not set up yet | This counter has not been paired with the shop yet, so it cannot take a sale. | Paste the pairing text from the owner's phone, choose a PIN and save. | a manager |  |
| pairingSaved | Counter set up | The pairing and the PIN are saved on this phone. | Keep the app open at the counter. It starts looking for the network now. | the cashier |  |
| pairingNotReadable | Pairing text not read | The pairing text is not in the form this app reads, so nothing was saved. | Copy the pairing text again from the owner's phone, then try again. | a manager |  |
| pinNotAccepted | PIN not accepted | A PIN is 6 to 12 digits, and the two entries must be the same, so nothing was saved. | Type a PIN of 6 to 12 digits twice, then save again. | a manager |  |
| nameNotAccepted | Name not accepted | A name is 1 to 64 characters long, so it was not saved. | Type the name again. | the cashier |  |
| torAddressNotAccepted | Tor address not accepted | The address of Orbot must be a host, a colon and a port, like 127.0.0.1:9050. | Type the address again. | a manager |  |
| torIsOn | Tor is on for this shop | The shop's settings send every connection of this counter through Tor, with Orbot. | Check the address of Orbot below, and keep Orbot running. | a manager |  |
| torIsOff | Tor is off for this shop | The shop's settings do not use Tor, so the price sources and the network can see this shop's address. | Nothing to do here. The owner can switch Tor on in the shop's settings. | the owner |  |
| orbotNotRunning | Tor is not running | Tor is on for this shop, but Orbot is not running. | Open Orbot and turn it on. | a manager |  |
| managerNeeded | A manager is needed | This step needs the manager's PIN. | Ask the manager to type the PIN. | a manager |  |
| pinWrong | Wrong PIN | That PIN is not right, so nothing was done. | Wait 4 seconds, then try again. | a manager |  |
| pinLocked | PIN locked for now | There were wrong tries in a row, so the PIN is locked for a while. | Wait 5 minutes, then try again. | a manager |  |
| priceReady | Price ready | Bitcoin is priced at 73,554.18 USD, from 3 price sources. | Nothing to do. The app asks the sources again every minute. | nobody |  |
| sourceNotAnswered | Price source not reached | A price source did not answer in time, so its price is missing this turn. | Nothing to do. The app asks again in a minute. | nobody |  |
| sourceRefusedStatus | Price source said no | A price source answered with a refusal, so its price is missing this turn. | Nothing to do. The app asks again in a minute. | nobody |  |
| sourceReplyTooLong | Price reply too long | A price source sent more than the app reads, so its price is missing this turn. | Nothing to do. The app asks again in a minute. | nobody |  |
| amountNeeded | Amount needed | Type an amount above zero to start a sale. | Press the digits of the amount, then press Open. | the cashier |  |
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
| fileNotRead | File not read | The file could not be opened or read, so nothing was done. | Pick the file again, or paste the text instead. | a manager |  |
| fileNotSaved | Not saved | The app could not write to this phone's storage, so nothing was saved. | Try again. If it happens again, stop and tell the owner in person. | a manager |  |
