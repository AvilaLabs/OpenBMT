# Screen rules

What a person is told, on every screen of every OpenBMT program. The aim
is that nobody using it is ever left not knowing what happened or what to
do. The rules have numbers (SC-xx) so that tests can be named after them.

**Status.** The shared type and its checker are built
(`crates/openbmt-core/src/say.rs`), and the words of the counter, the
owner's libraries, the Key app and the Box console are written and
listed in `docs/screens/`. Owner-device coin, sending and recovery states
also have words; its remaining link and refund-intake alerts and notes
still need coverage. The words have passed the program's check
only: no person outside the project has read them, and no app screen
is drawn yet.

## Who reads a screen

| Person | Program | Knows |
|---|---|---|
| Cashier | Counter app | How to sell. Nothing about bitcoin. |
| Manager | Counter app | The shop. Little about bitcoin. |
| Owner | Owner app | The shop and its money. Some bitcoin. |
| Key holder | Key app | What they were told when given the key. |
| Box keeper | Box console | How to type a command. |

A screen is written for the person in its row and for nobody who knows
more.

## The rules

- **SC-01. One state, one screen.** Every state a person can be in, and
  every refusal a person can get, has exactly one `Words`: a heading, one
  sentence that says what happened, one sentence that says what to do
  next, and who does it. The code that makes them matches on every
  variant with no catch-all arm, so a new state cannot be added without
  its words.
- **SC-02. There is always a next step.** The second sentence is never
  missing. When nothing is to be done it says so and says what is being
  waited for ("Nothing to do. It turns green when the payment is in a
  block, in about ten minutes."). A screen never ends at "it failed".
- **SC-03. No notation of the program.** The heading and the two
  sentences hold no rule number, no key, no transaction id or other run
  of hex, no time in seconds, no name of a function or a type, and no
  text the operating system gave. The rule number goes in `code` and the
  system's text in `detail`; an app shows both only under "Details". A
  time goes in `at` and the app shows it on the person's own clock. A
  device is named by the name the owner gave it, never by its key.
- **SC-04. Plain words.** The words in `JARGON` (`say.rs`) are not used.
  Where the rules documents say "change mark", "receive mark", "dust
  line", "slice" or "exposure", a screen says what it means for the
  person: "the last address this Key app has seen", "too small to be
  worth sending", "the amount this counter may accept before a block".
- **SC-05. Who does it.** Every `Words` names who takes the next step:
  nobody, the customer, the cashier, a manager, the owner, the key
  holder or the Box keeper. When the person reading cannot fix it, the
  sentence says who can and what to tell them, and what the reader does
  meanwhile ("Take another kind of payment. Tell the owner: this counter
  needs new addresses.").
- **SC-06. Four colours, one meaning each.** Blue: nothing is wrong and
  it is not finished. Green: done; at a counter, hand over the goods.
  Red: stop; at a counter, do not hand over the goods. Neutral: ended
  with nothing received. Red is never used for waiting, and green never
  for something that can still be undone without saying so in the first
  sentence.
- **SC-07. Different causes read differently.** Two states of one
  program never share a heading and a first sentence. "Waiting because
  the amount is over this counter's limit" and "waiting because
  acceptance at once is switched off" are two screens. The same fault
  reached two ways is one sentence, not two.
- **SC-08. Text from elsewhere is marked.** Text typed on another
  device, read from a scanned code or sent by a point-of-sale system is
  never part of the heading or the sentences. It is shown in quotation
  marks under a line that says where it came from, with control
  characters and characters that hide or turn text around removed, and
  cut to 64 characters (`clean_foreign`).
- **SC-09. Money is written to be read.** An amount is given in the
  shop's currency when the state knows it, and otherwise in sats with
  separators ("12,345 sats"). An amount is said once, not in both
  sentences.
- **SC-10. The program's own faults have one screen.** A fault that is
  not the person's doing and that the person cannot fix (storage that
  failed, a file that does not read back, a check inside the program
  that failed) says three things: nothing was changed or what was, what
  to do now (try again, or stop and tell the owner), and that the detail
  is under "Details". It does not say "internal error". There is no
  support desk behind OpenBMT, so no screen sends anyone to one: a
  fault the owner cannot fix is reported to the OpenBMT project.
- **SC-11. The console keeps the same rules.** Every answer of the Box
  console is the heading on one line, what happened on the next, then
  "Next:" and the step, then "Details:" with the code when there is
  one. Lines a script reads come after them and keep their form.
  `state` never groups two conditions in one line.
- **SC-12. A test for every program.** Each program has a function that
  returns the `Words` of every state and refusal it can show, with at
  least one value for each variant and for each variant inside it. One
  test passes that list to `check_distinct`, which applies SC-02 to
  SC-04, SC-07 and SC-08 as far as a program can check them. A second
  test fails when a variant is missing from the list.

## What the programs keep

The `Display` texts that begin with a rule number stay as they are. They
are for logs, for the notes a program keeps, and for tests named after
rules. A person is shown `Words`.

## What a program cannot check

`check` finds notation, jargon, length and sameness. It cannot tell
whether a sentence is true, kind, or enough to act on. Those are read by
a person before a release, with the list SC-12 produces printed in full,
and they are tried with a cashier who has not seen the app before. That
trial has not happened.
