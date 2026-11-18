# Box console: what a person is told

Made by `screens_markdown` in `crates/openbmt-box/src/say.rs`. Do not
edit it by hand: run the test `sc_12_the_list_is_on_disk` with the
environment variable `OPENBMT_WRITE_SCREENS=1` to write it again.

| State | Heading | What happened | What to do next | Who | Code |
|---|---|---|---|---|---|
| BoxStatus::Unpaired | Not paired yet | This Box is not paired with a shop and holds no invitation. | Make an invitation in the Key app, then type invite and the invitation here. | the Box keeper | BX-03 |
| BoxStatus::InvitationOpen | Invitation open | This Box holds an invitation and waits for the owner's phone to pair it. | Finish pairing from the owner's phone within 30 minutes of typing the invitation. | the owner | BX-12 |
| BoxStatus::Paired | Paired | This Box is paired and the shop's settings list it. | Nothing to do. It passes messages between the shop's devices. | nobody | BX-16 |
| BoxStatus::Unpaired on the page | Waiting for an invitation | This Box is not paired with a shop and is waiting for an invitation. | Make an invitation in the Key app, then type it in the box below. | the Box keeper | BX-03 |
| PageAnswer::InvitationHeld | Invitation already held | This Box already holds an invitation that has not run out, so it took no other. | Finish pairing from the owner's phone, or wait until the invitation runs out after 30 minutes and enter a new one. | the Box keeper | BX-57 |
| PageAnswer::NotFromThisPage | Not sent from this page | The invitation did not come from this page, so the Box did not take it. | Open this page in the browser again and type the invitation in the box on it. | the Box keeper | BX-58 |
| PageAnswer::SetupWrong | Setup code wrong | The setup code does not match the one in this Box's log. | Wait ten seconds, then type the code from the log again. | the Box keeper | BX-62 |
| BoxStatus::TurnedOff and BoxError::TurnedOff and NoteKind::TurnedOff | Turned off | The owner's settings no longer list this Box, so it is turned off. | To use it again, erase its data folder and pair it from the owner's phone with a new invitation. | the Box keeper | BX-23 |
| BoxError::Damaged and StartFailure::State: key | File damaged | The file box.key is damaged, so the Box did not start and did not replace it. | Restore the file from a copy, or erase the data folder and pair the Box again. | the Box keeper | BX-51 |
| BoxError::Damaged and StartFailure::State: roots | File damaged | The file roots is damaged, so the Box did not start and did not replace it. | Restore the file from a copy, or erase the data folder and pair the Box again. | the Box keeper | BX-51 |
| BoxError::Io and StartFailure::State | Storage problem | The Box could not write the file settings and kept what it had. | Check that the disk has room and this user may use the data folder, then try again. | the Box keeper | BX-50 |
| BoxError::NoRandomness | No random numbers | The computer gave the Box no random numbers, so it changed nothing. | Try the command again; if it keeps happening, the computer's random source needs repair. | the Box keeper | BX-01 |
| BoxError::AlreadyPaired | Already paired | This Box is already paired with a shop, so it took no invitation. | To pair it again, erase its data folder and start the Box again. | the Box keeper | BX-16 |
| BoxError::Invitation and InviteTextError::BadLength | Invitation not read | An invitation has 30 characters and this one has 12. | Type it again exactly as the Key app shows it. | the Box keeper | BX-10 |
| BoxError::Invitation and InviteTextError::BadCharacter | Invitation not read | One character is not a character an invitation uses. | Type it again exactly as the Key app shows it. | the Box keeper | BX-10 |
| BoxError::Invitation and InviteTextError::BadCheck | Invitation not read | The last characters of the invitation do not match the rest, so one was mistyped. | Type it again, checking each group of five characters. | the Box keeper | BX-10 |
| BoxError::NoInvitation | No invitation open | This Box holds no invitation, so it cannot take a welcome. | Type invite and an invitation from the Key app, then pair from the owner's phone. | the Box keeper | BX-12 |
| BoxError::InvitationExpired | Invitation ran out | The invitation was older than 30 minutes, so the Box forgot it. | Make a new invitation in the Key app and type it with the invite command. | the Box keeper | BX-12 |
| BoxError::BadWelcome | Welcome not read | What the owner's phone sent to pair this Box is not a welcome. | Pair again from the owner's phone while the invitation is still open. | the owner | BX-15 |
| BoxError::WelcomeMac | Welcome is not for this Box | The welcome was not made for the invitation typed into this Box. | Check that the owner's phone used the invitation typed here, then pair again. | the owner | BX-15 |
| BoxError::BadSettings | Settings not read | The settings that arrived are not settings the Box can read. | Send the settings again from the owner's phone. | the owner | BX-15 |
| BoxError::Settings and SettingsError::OwnerSignatureInvalid | Settings not signed | The new settings do not carry the owner's signature, so the Box kept its own. | Send the settings again from the owner's phone. | the owner | BX-21 |
| BoxError::Settings and SettingsError::ApproverRequired | Settings need the Key app | The new settings loosen something and need the Key app's approval too. | Approve the change in the Key app, then send the settings again. | the owner | BX-21 |
| BoxError::Settings and SettingsError::ApproverSignatureInvalid | Key app approval is wrong | The Key app's approval on the new settings does not check, so the Box kept its own. | Approve the change in the Key app again, then send the settings again. | the owner | BX-21 |
| BoxError::Settings and SettingsError::StaleVersion | Settings are not newer | The new settings are no newer than the settings the Box already has. | Send the newest settings from the owner's phone. | the owner | BX-21 |
| BoxError::Settings and SettingsError::Invalid and BoxError::Confirmed | Settings break a rule | The new settings break one of the rules every settings must keep. | Change the settings on the owner's phone and send them again. | the owner | BX-21 |
| BoxError::NotListed | Box not in the settings | The settings the owner sent do not list this Box as a Box, so pairing did not finish. | Add this Box to the shop's devices on the owner's phone, then type a new invitation here. | the owner | BX-15 |
| BoxError::TooManyMembers | Too many devices | The shop's device list is longer than the 64 devices a Box can serve. | Remove a device on the owner's phone and send the settings again. | the owner | BX-22 |
| BoxError::NotPaired | Pair the Box first | This Box is not paired with a shop, so it did nothing. | Pair it first: type invite and the invitation, then approve it from the owner's phone. | the Box keeper | BX-21 |
| StartFailure::Lock and LockError::Busy | Data folder in use | Another OpenBMT program is already using this Box's data folder. | Stop the other program, or give this Box its own data folder, then start it again. | the Box keeper | BX-02 |
| StartFailure::Lock and LockError::Io | Data folder not locked | The Box could not lock its data folder, so it did not start. | Check that the folder exists and this user may write in it, then start the Box again. | the Box keeper | BX-02 |
| StartFailure::Relay | Relay files not opened | The Box could not open the files of its message relay, so it did not start. | Check the relay folder in the data folder and that the disk has room, then start the Box again. | the Box keeper | LK-41 |
| StartFailure::Courier and StartError::NeedsNewPairing | Link file missing | The link file link-inbox is missing, so the Box did not start. | Erase the data folder and pair the Box again from the owner's phone. | the Box keeper | LK-53 |
| StartFailure::Courier and StartError::Corrupt | Link file damaged | The link file link-outbox does not read back, so the Box did not start and did not replace it. | Restore the file from a copy, or erase the data folder and pair the Box again. | the Box keeper | LK-53 |
| StartFailure::Courier and StartError::Store | Link files not usable | The Box could not read the link file link-inbox, so it did not start. | Check that the disk works and this user may use the data folder, then start the Box again. | the Box keeper | LK-40 |
| StartFailure::Listen | Box could not listen | The Box could not listen on the address it was given, so it did not start. | Check that no other program uses that port and the address is this computer's, then start again. | the Box keeper |  |
| ConsoleAnswer::InvitationTaken | Invitation taken | The Box took the invitation and keeps it for 30 minutes. | Finish pairing from the owner's phone before the time is up. | the owner | BX-12 |
| ConsoleAnswer::Members: none | No devices served | The Box serves no devices, because it is not paired or it is turned off. | Run the state command to see which, and follow what it says. | the Box keeper | BX-22 |
| ConsoleAnswer::Members: some | Devices on the list | The Box serves 3 devices. | Nothing to do. The owner's settings decide who is on the list. | nobody | BX-22 |
| ConsoleAnswer::NoNotes | No notes | The Box has kept no notes. | Nothing to do. Run notes again later to see new ones. | nobody |  |
| ConsoleAnswer::UnknownCommand | Command not known | The Box does not know that command. | Type one of invite, state, members, notes or quit. | the Box keeper |  |
| ConsoleAnswer::InvitationFileUnreadable | Invitation file not read | The Box could not read the invitation file it was given. | Check the file name and that this user may read it, then use the invite command. | the Box keeper | BX-12 |
| ConsoleAnswer::BadArguments | Wrong arguments | The Box needs a data folder and an address to listen on, and may take other options. | Type the data folder and the address, then add --invitation-file, --status-page or --no-console as needed. | the Box keeper |  |
| ConsoleAnswer::BadAddress | Address not understood | The address to listen on is not an address and a port. | Give it as an address, a colon and a port, such as 127.0.0.1:8080. | the Box keeper |  |
| NoteKind::MemberListNotReplaced | Device list not updated | The Box could not change the list of devices it serves, so the old list stays. | Check the disk and the data folder; the Box tries again at its next step. | the Box keeper |  |
| NoteKind::LinkFiles | Link files not usable | The Box could not use its link files, so it is not passing messages. | Check the disk; if it keeps happening, erase the data folder and pair the Box again. | the Box keeper |  |
| NoteKind::MessageNotHandled | Message not handled | A message of a kind this Box does not handle arrived, and it was dropped. | Nothing to do. This version of the Box does not use that kind of message. | nobody |  |
| NoteKind::SettingsRefused | Settings refused | New settings from the owner were refused, so the Box kept the settings it had. | Tell the owner that the Box refused them, and say what is under Details. | the owner |  |
| NoteKind::Other | Note from the Box | The Box kept a note that has no plain words yet. | Keep the text under Details for whoever looks after this Box. | the Box keeper |  |
