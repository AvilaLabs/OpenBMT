# Key app: what the app itself tells a person

Made by `AppWords.screensMarkdown` in
`apps/key-android/src/model/AppWords.kt`. Do not edit it by hand: run
`apps/key-android/tests/run-jvm.sh` with the environment variable
`OPENBMT_WRITE_SCREENS=1` to write it again.

Everything the rules refuse is shown with the words of the library
(`docs/screens/key.md`, `openbmt-key-ffi`). These are the words of what
the app decides itself, with the key ceremony screens (`docs/DESIGN.md`
section 2.1, steps 2 and 5) among them.

| State | Heading | What happened | What to do next | Who | Code |
|---|---|---|---|---|---|
| debuggingOn | USB debugging is on | USB debugging is switched on, so a computer could read this phone. | Switch USB debugging off in the developer settings, then open this app again. | the key holder |  |
| wirelessDebuggingOn | Wireless debugging is on | Wireless debugging is switched on, so a computer on the network could read this phone. | Switch wireless debugging off in the developer settings, then open this app again. | the key holder |  |
| accessibilityOn | A screen helper is on | An accessibility service is switched on, and it could read or press things in this app. | Switch off every accessibility service in the settings, then open this app again. | the key holder |  |
| keyInSoftware | Key kept in software | This phone has no protected chip for keys, so the saved key is held by software alone. | Use this phone for tests only. For real funds, use a phone with a protected key chip. | the key holder |  |
| libraryMissing | The app is incomplete | The part of this app that holds the key did not load, so nothing was done. | Do not use this copy. Install the app again from the place you got it. | the key holder |  |
| fault | Answer not readable | The app could not read an answer from its own key part, so nothing was changed. | Close the app and open it again. If it happens again, stop and tell the owner in person. | the key holder |  |
| keystoreUnavailable | Key chip not usable | The phone's key storage did not start, so the saved key was not opened. | Restart the phone and open this app again. If it happens again, stop and tell the owner in person. | the key holder |  |
| fileNotRead | File not read | The file could not be opened or read, so nothing was done. | Pick the file again, or ask the owner to send it again. | the key holder |  |
| fileNotSaved | File not saved | The answer file could not be written, so the owner's phone has not got it yet. | Save the file again, in a place the owner's phone can read. | the key holder |  |
| cameraOff | Camera is off for this app | This app is not allowed to use the camera, so it cannot scan a code. | Allow the camera for this app in the phone's settings, or paste the text or choose a file instead. | the key holder |  |
| cameraNotStarted | Camera did not start | The camera could not be opened, so nothing was scanned. | Close any other app that uses the camera and try again, or paste the text or choose a file instead. | the key holder |  |
| codeNotForField | Not a text code | This code is part of a request or an answer, and this field takes plain text. | Scan the plain text code, or paste the text or choose a file instead. | the key holder |  |
| notANumber | Check what you typed | One of the amounts is empty or has something other than digits in it. | Type whole numbers of sats only, then try again. | the key holder |  |
| flipsNotReadable | Check the coin flips | The flips may hold only H for heads and T for tails. | Remove any other letter or digit, then try again. | the key holder |  |
| answersReady | Answers ready | Every line has an answer, and the answer file is signed and ready on this phone. | Save the answer file, then take it to the owner's phone. | the key holder |  |
| pairOffer | Pair a counter | The code below names this shop's two keys, its wallet and its network, for a counter to read. | Scan this with the counter: open the counter app, tap Pair with the Key app, and hold its camera to this code. | the key holder |  |
| pairBox | Pair a Box | The code below lets one Box join this shop. It works once and runs out in 30 minutes. | Type all 30 characters into the Box's own setup page. Do not type them on a shared computer, and do not send or photograph them. | the key holder |  |
| invitationRunOut | Invitation run out | The 30 minutes are over, so a Box can no longer join with that code. | Make a new invitation if a Box still has to be paired. | the key holder |  |
| boxFound | Box found | A Box has asked to join this shop. The four words are made from the Box's key. | Approve the settings that add it first. Then approve only if its four words match the Box's own screen, in the same order. | the key holder |  |
| choosePin | Choose a PIN | The key on this phone is kept behind a PIN, which the app asks for every time it opens. | Choose at least 6 digits, or a passphrase of at least 8 characters, and do not forget it. | the key holder |  |
| upgradePin | Set a PIN for this key | The key on this phone was saved without a PIN, so anyone who opens the app could use it. | Choose a PIN now; the key is kept as it is and locked behind it. | the key holder |  |
| pinsDiffer | The two PINs differ | The PIN typed the second time is not the same as the first, so nothing was set. | Type the same PIN in both fields, then try again. | the key holder |  |
| pinChanged | PIN changed | The key on this phone is now locked behind the new PIN. | Nothing to do. Use the new PIN from now on. | nobody |  |
| working | Working | The app is opening the key, which takes a moment. | Wait. Do not close the app. | nobody |  |
| waitForTry | Wait before the next try | There were too many wrong tries, so the app waits before it takes another. | Wait for the time shown, then type the PIN again. | the key holder |  |
| triesNotSaved | Try not counted | The phone could not keep the count of wrong tries, so the PIN was not tried. | Try again. If it keeps failing, stop and tell the owner in person. | the key holder |  |
| wiped | Key wiped from this phone | This phone's copy of key 2 was wiped after 10 wrong tries. The money is not lost: the other two keys still exist. | The wallet needs two of three keys. Restore key 2 from its 24 words on this phone, or tell the owner in person. | the key holder |  |
| wipeFailed | Wipe not finished | The key could not be wiped from this phone completely, so the app will try again when it opens. | Close the app and open it again. If it keeps failing, uninstall the app. | the key holder |  |
| cashOut | Cash-out addresses | Payments to these addresses count against the higher limits, once the 24 hours of waiting are over. | Add an address by scanning the deposit code the exchange shows with this phone's camera. | the key holder |  |
| scanDeposit | Scan the deposit code | Hold this phone's camera to the deposit code the exchange shows. An address cannot be typed or pasted here. | Then type a name for it, and check the address on the next screen. | the key holder |  |
| notADepositCode | Not a deposit code | This code is part of a request or an answer, not a deposit code, so nothing was taken. | Scan the deposit code the exchange shows. | the key holder |  |
| nameAddress | Name this address | Type a name for the address, such as the exchange's name, so you can tell it apart later. | Then check the address on the next screen. | the key holder |  |
| checkAddress | Check this address | Compare every group of four with what the exchange shows. One wrong character sends money to the wrong place. | If they all match, add it. It can be used 24 hours from now, by this phone's clock. | the key holder |  |
| addressAdded | Address added | The address is saved, and it counts as a cash-out address in 24 hours, by this phone's clock. | Nothing to do. Addresses can be added or removed here at any time. | nobody |  |
| removeAddress | Remove this address? | Removing it takes effect at once: payments to it count as payments to any other address. | Remove it only if it is not needed. It can be added again, and then it waits 24 hours. | the key holder |  |
| addressRemoved | Address removed | The address is no longer a cash-out address, and the change took effect at once. | Nothing to do. | nobody |  |
| key3Needed | Key 3, the recovery key | Key 3 is made on its own in another publisher's wallet or on a hardware wallet, never in this app. | Do that step alone, write its words on separate paper, then scan key 3 here: a key code from the other wallet app, or its key as text. | the key holder |  |
| key3UrNotRead | Code is read by the camera | A key code is read only when it is scanned, not from pasted text, so nothing was taken. | Scan the key code, or paste the key as text. | the key holder |  |
| key3NotText | Code is not text | The code holds something other than text, so it cannot be key 3 and nothing was taken. | Export key 3 as text in the other wallet, then scan it again, or paste the text. | the key holder |  |
| key3Taken | Key 3 taken | This app now holds the public part of key 3, and nothing else of it. | Compare the short code shown here with the one in the other wallet app, then go on. | the key holder | SR-67 |
| key3Mismatch | Key 3 does not match | The fingerprint here is not the one the other wallet shows, so this is not the key 3 that was made. | Export key 3 again from the other wallet and scan it again; nothing goes on until they match. | the key holder |  |
| publicAnswerReady | Answer ready for the owner | This app shows its public details, which the owner's phone needs to build the wallet. | Scan the answer with the owner's phone, then wait for the wallet code. | the owner | SR-69 |
| scanOwnerWallet | Scan the owner's wallet | The owner phone builds the wallet from the keys it scanned, then shows it as a code. | Scan that code here, then compare the words and fingerprints it shows. | the key holder |  |
| checkWallet | Check the wallet | The wallet was read, and its four words and three fingerprints are below. | The owner phone and the counter must show these same words and fingerprints. | the key holder |  |
| walletMismatch | Wallet words differ | The words or fingerprints here differ from the owner phone's, so this is not the wallet to use. | Nothing was registered. Check the owner phone, then scan its wallet code again. | the key holder |  |
| walletLacksKey3 | Wallet lacks this key 3 | The owner phone's wallet does not hold the key 3 scanned here, so nothing was accepted. | Do not go on. Tell the owner in person, then make the wallet again on the owner phone. | the key holder |  |
| Empty | No key yet | This app has no key yet. | Make the key, or type the words of an existing key. | the key holder |  |
| HasIdentity(WordsWaiting) | Write the words down | The words of this key are ready, and they are shown only once. | Write them on paper now; they cannot be shown again. | the key holder |  |
| HasIdentity(Key2) | Waiting for the wallet | This app holds key 2 and is waiting for the wallet from the owner's phone. | Give the owner's phone this app's key details, then accept the wallet it makes. | the key holder |  |
| HasIdentity(NoKey2) | Waiting, without key 2 | This app has its own identity but not key 2, and is waiting for the wallet. | Give the owner's phone this app's identity, then accept the wallet it makes. | the key holder |  |
| Locked | Enter your PIN | The key on this phone is kept behind a PIN. | Type the PIN to unlock the app. | the key holder |  |
| WalletPending | Wallet waiting for you | The wallet from the owner's phone is held but not registered, so nothing can be signed yet. | Compare its words and fingerprints with the owner's phone, then confirm or discard it. | the key holder |  |
| Ready(Key2) | Ready to approve | The wallet is accepted, and this app can answer the owner's requests. | Nothing to do. It waits for a request from the owner's phone. | nobody |  |
| Ready(NoKey2) | Ready, without key 2 | The wallet is accepted, but this phone does not hold key 2. | Nothing to do for payments here. This app answers other requests only. | nobody |  |
| Passed | Check passed | This check of the line passed. | Nothing to do for this check. | nobody |  |
| Failed | Check not passed | This check of the line did not pass, so the line cannot be approved. | Decline the line, and tell the owner in person which check it was. | the key holder |  |
| NotChecked | Check not run | An earlier check did not pass, so this one was not run. | Look at the first check that did not pass. | the key holder |  |
| Kit Same | Kit matches this phone | The wallet on the paper is the same as the wallet on this phone. | Nothing to do. Only the wallet is compared, not the other lines on the paper. | nobody |  |
| Kit SameCheckWords | Same check words, other keys | The paper shows the same four words but another wallet, which should not happen by accident. | Stop, do not use this kit, and tell the owner in person. | the key holder |  |
| Kit NotAWallet | Paper is not a wallet | The text scanned from the paper is not a wallet this app can read. | Scan the wallet code again; if it still differs, do not use this kit and print a new one. | the key holder |  |
| Kit OtherWallet | Kit is for another wallet | The paper and this phone do not describe the same wallet. | Do not use this kit to recover; print a new kit from the owner's phone and check it. | the key holder |  |
| UrWords.reading | Reading the animated code | Reading the animated code: 3 of about 9 parts. | Keep the phone pointed at the code until it is done. | the key holder |  |
| UrWords.restarted | Code did not add up | The parts read did not fit together, so reading starts again. | Keep the phone pointed at the code. It reads from the first part again. | the key holder |  |
| UrWords.otherCode | Another code is in view | The code in view is not part of the one being read, so it is skipped. | Point the phone at the first code again, or cancel and start over. | the key holder |  |
| UrWords.wrongKind | Not the code expected | This code carries something this step does not take, so nothing was used. | Cancel, then ask for the right code again, or use a file instead. | the key holder |  |
| UrWords.contentRefused | Code content not usable | The code was read in full, but what it holds is not in a form this step takes. | Ask for the code to be shown again, or use a file instead. | the key holder |  |
| UrWords.readerFailed | Code reader failed | The part of this app that reads codes gave no usable answer, so nothing was read. | Close the app and open it again. If it happens again, use a file. | the key holder |  |
| UrWords.showingAnimated | Answer shown as a code | The answer is on the screen as a code that changes, one part after another. | Hold the other phone to the screen until it has read every part. | the key holder |  |
| UrWords.showingStill | Answer shown as one code | The answer is on the screen as one code that does not change. | Hold the other phone to the screen until it has read the code. | the key holder |  |
| UrWords.cannotShow | No code could be made | The answer could not be turned into a code, so nothing is shown. | Save the answer as a file instead. | the key holder |  |
