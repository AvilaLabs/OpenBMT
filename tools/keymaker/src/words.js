// Every sentence the key maker page shows, in one place. The page reads its
// text only from here, so the words can be reviewed without reading code.
// `{n}`, `{total}`, `{pos}` and similar are filled in by the page.

export const WORDS = Object.freeze({
  title: 'OpenBMT Key Maker',
  tagline: 'Your coin or die, your words. The inconvenience happens once.',
  label:
    'Internal review only. No independent security audit has been done yet.',
  rehearsalBanner: 'REHEARSAL. These words are made up and will be thrown away.',
  stepBadge: 'Step {n} of {total}',
  next: 'Next',

  // Step 0
  startHeading: 'Before you start',
  startChecklist: [
    'Tails is running from the USB stick.',
    'You turned networking off on the Tails start screen.',
    'You have a coin or one six-sided die, a pen and the blank card.',
    'Nobody can see your screen or your card.',
    'You have about an hour.',
  ],
  startOffline:
    'This page cannot reach the internet. It stores nothing and forgets ' +
    'everything when you close it.',
  rehearseButton: 'Rehearse first',
  rehearseNote:
    'Go through every step once with made-up results. Nothing from the ' +
    'rehearsal is kept. Then start again for real.',
  realButton: 'Start for real',

  // Step 1 is the rehearsal itself; it uses the same screens.
  rehearsalDoneHeading: 'Rehearsal finished',
  rehearsalDoneText:
    'That is the whole process. Cross out anything you wrote during the ' +
    'rehearsal. Now do it again for real.',

  // Step 2: the choice
  methodHeading: 'Coin or die',
  methodText:
    'Both are equally good. Use the same coin, or the same die, from ' +
    'start to finish.',
  methodCoin: 'Coin',
  methodCoinNote: 'About 512 flips. Flips are read in pairs.',
  methodDie: 'One six-sided die',
  methodDieNote: 'About 270 rolls. Rolls are read in threes.',

  // Step 2: the coin
  flipHeading: 'Flip your coin',
  flipText:
    'Flip the coin and press what it landed on. Keys H and T work too.',
  flipPairs:
    'Each pair of flips gives one bit. Heads then tails gives 0, tails ' +
    'then heads gives 1, and two the same are dropped. This cancels out a ' +
    'coin that lands one way more often.',
  flipCount: '{n} of {total} bits',
  flipsMade: '{n} flips so far',
  heads: 'Heads',
  tails: 'Tails',
  undo: 'Undo last flip',
  flipMistake:
    'Pressed the wrong one? Undo it and press what the coin showed.',
  flipsDone: 'All 128 bits done',
  rehearsalFill: 'Fill with made-up results',

  // Step 2: the die
  dieHeading: 'Roll your die',
  dieText:
    'Roll the die and press the number on top. Keys 1 to 6 work too.',
  dieTriples:
    'Each three rolls give one value. Three different numbers give a ' +
    'value from their order, low, middle and high; three with a number ' +
    'repeated are dropped. This cancels out a die that favours some ' +
    'numbers.',
  dieOrders:
    'Low middle high 1, low high middle 2, middle high low 3, high middle ' +
    'low 4, high low middle 5, middle low high 6.',
  dieCount: '{n} of {total} values',
  dieRollsMade: '{n} rolls so far',
  dieUndo: 'Undo last roll',
  dieMistake:
    'Pressed the wrong one? Undo it and press what the die showed.',
  dieDone: 'All 50 values done',

  // Step 3
  writeHeading: 'Write your words',
  writeText:
    'Write each word on the card by hand, in order, with its number. ' +
    'Never type them anywhere else, never photograph them, never print them.',
  writeSteel: 'Then put them on steel as soon as you can.',
  writeDone: 'I have written them all',

  // Step 4
  checkHeading: 'Check your card',
  checkText:
    'Type the first four letters of these words from your card, not from ' +
    'memory.',
  checkWordLabel: 'Word {pos}',
  checkButton: 'Check',
  checkWrong:
    'Word {pos} does not match. Look at your card and correct it there ' +
    'before you go on.',
  checkBack: 'Show the words again',

  // Step 5
  phoneHeading: 'Show your phone',
  phoneText:
    'In the OpenBMT app, scan this code. The phone must show the same ' +
    'fingerprint and first address as this page.',
  phonePublic:
    'This code is your wallet’s public description. It lets the phone ' +
    'see payments and make addresses. It cannot spend.',
  fingerprintLabel: 'Fingerprint',
  addressLabel: 'First address',
  phoneDone: 'The phone shows the same',

  // Step 6
  electrumHeading: 'Check with Electrum',
  electrumText:
    'Electrum is made by other people, so it is an independent check. ' +
    'It must show the same first address and fingerprint as this page.',
  electrumSteps: [
    'Open the Applications menu, then Internet, then Electrum.',
    'Tails warns that persistence is off and the data will be lost. That ' +
      'is what you want. Click Launch, then I Accept.',
    'Keep the wallet name, click Next. Choose Standard wallet, Next. ' +
      'Choose I already have a seed, Next.',
    'Click Options, choose BIP39 seed, click OK.',
    'Type your 12 words from the card, then read them against the card. ' +
      'Click Next. If the first click only closes the word list, click it ' +
      'again.',
    'Choose native segwit (p2wpkh). Leave the path as it is. Next.',
    'Leave the password empty. Click Finish.',
    'Open Wallet, then Information: the BIP32 root fingerprint must be ' +
      'the one on this page. Open the Receive tab and press Request: the ' +
      'address must be the one on this page.',
  ],
  electrumClose: 'Close Electrum without saving the wallet.',
  electrumDone: 'Electrum shows the same',
  electrumDifferent: 'Electrum shows something different',
  electrumStop:
    'Stop here. Do not use these words. Close this page, shut down Tails ' +
    'and start again from the beginning.',

  // Step 7
  finishHeading: 'Shut down',
  finishText:
    'Press Clear and close, then shut down Tails. Everything on this ' +
    'computer is forgotten.',
  finishCard:
    'Keep the card somewhere safe and private. Put the words on steel as ' +
    'soon as you can.',
  clearButton: 'Clear and close',
  cleared: 'Cleared. You can shut down Tails now.',

  // Check it by hand
  handHeading: 'Check it by hand',
  handText:
    'Your bits, in groups of 11. Each group is a number from 0 to 2047, ' +
    'and that number picks the word from the BIP39 word list, counting ' +
    'from 0.',
  handLast:
    'The last word uses your last 7 bits, then 4 check bits made from all ' +
    'your bits.',
  handCheckCommand:
    'To check the 4 check bits in Tails, open Applications, then System ' +
    'Tools, then Console, and type the line below. The first character of ' +
    'the answer, in binary, is the check bits.',
  handDieText:
    'Your 50 values as one line of digits. Your bits are the first 128 ' +
    'bits of its SHA-256.',
  handDieCommand:
    'To check this in Tails, open Applications, then System Tools, then ' +
    'Console, and type the line below. Read it against the page before ' +
    'you press Enter: one wrong digit gives a different answer. The first ' +
    '32 characters of the answer must be the ones shown here.',
  handDieValues: 'Values',
  handDieHash: 'First 32 characters',
  handGroup: 'Group {n}',
  handNumber: 'Number',
  handWord: 'Word',
  handCheckBits: 'Check bits',
  handFlips: 'Bits',

  // Shown in place of the page when something is wrong
  noScript:
    'This page needs JavaScript. In the Tor Browser, set the security level ' +
    'to Standard, then reload the page.',
  inert:
    'The page did not start. Reload it once. If it still does not start, ' +
    'check the security level is Standard.',
  failed:
    'Something went wrong and the page stopped. Nothing was kept. Close ' +
    'the page and start again.',
});
