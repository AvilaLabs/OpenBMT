// Every sentence the key maker page shows, in one place. The page reads its
// text only from here, so the words can be reviewed without reading code.
// `{n}`, `{total}`, `{pos}` and similar are filled in by the page.

export const WORDS = Object.freeze({
  title: 'OpenBMT Key Maker',
  tagline: 'Your coin, your words. The inconvenience happens once.',
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
    'You have a coin, a pen and the blank card.',
    'Nobody can see your screen or your card.',
    'You have about an hour.',
  ],
  startOffline:
    'This page cannot reach the internet. It stores nothing and forgets ' +
    'everything when you close it.',
  rehearseButton: 'Rehearse with made-up flips',
  rehearseNote:
    'Go through every step once with made-up flips. Nothing from the ' +
    'rehearsal is kept. Then start again for real.',
  realButton: 'Start for real',

  // Step 1 is the rehearsal itself; it uses the same screens.
  rehearsalDoneHeading: 'Rehearsal finished',
  rehearsalDoneText:
    'That is the whole process. Cross out anything you wrote during the ' +
    'rehearsal. Now do it again for real, with your coin.',

  // Step 2
  flipHeading: 'Flip your coin',
  flipText:
    'Flip the coin and press what it landed on. Do it 128 times. ' +
    'Keys H and T work too.',
  flipCount: '{n} of {total}',
  heads: 'Heads',
  tails: 'Tails',
  undo: 'Undo last flip',
  flipMistake:
    'Pressed the wrong one? It does no harm: it is still random. ' +
    'Undo only if you want to.',
  flipsDone: 'All 128 flips done',
  rehearsalFill: 'Fill with made-up flips',

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
    'Follow the steps on your phone. Electrum must show the same first ' +
    'address and fingerprint.',
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
    'Your flips, in groups of 11. Each group is a number from 0 to 2047, ' +
    'and that number picks the word from the BIP39 word list, counting ' +
    'from 0.',
  handLast:
    'The last word uses your last 7 flips, then 4 check bits made from ' +
    'all your flips.',
  handGroup: 'Group {n}',
  handNumber: 'Number',
  handWord: 'Word',
  handCheckBits: 'Check bits',
  handFlips: 'Flips (heads 1, tails 0)',

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
