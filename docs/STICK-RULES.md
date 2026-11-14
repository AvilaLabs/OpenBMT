# Stick rules: the shop's key made on an ordinary computer, offline

Status: specification, written before the code. Each rule has an ID so that
a test can name the rule it proves (`sk_NN_...`).

The stick is a folder the owner copies onto any USB stick. An ordinary PC
started from it runs one program and nothing else: the key maker. The
computer's own system never starts, no network is ever turned on, and
nothing is written anywhere. The owner leaves with the wallet's words in
their own hand and the wallet's public description on the owner phone.

The stick makes the key, and signs the rare payment from savings: a refill
of the refund float, a cash-out. Everyday refunds are paid by a hardware
signer or from the refund float, a small capped wallet of the shop's own.

**First build. No independent security audit has been done. Not for real
funds.**

## 1. What is on the stick

- **SK-01. The folder.** The stick holds exactly: `EFI/BOOT/BOOTX64.EFI`
  (the distribution's shim, signed for Microsoft's third-party UEFI CA),
  `EFI/BOOT/grubx64.efi` (the distribution's signed GRUB),
  `EFI/BOOT/grub.cfg` and the same file at the distribution's own GRUB
  path, `vmlinuz` (the distribution's signed kernel), `initrd.img` (ours)
  and `SHA256SUMS`. Nothing else.
- **SK-02. Inputs from the distribution, pinned.** Every file that is not
  ours comes from a package of the distribution's archive named in
  `stick/inputs.lock` with its version and SHA-256. The build downloads only
  those packages, from that archive, and refuses a package whose hash
  differs. No file of a package is changed.
- **SK-03. Ours.** `grub.cfg` boots `vmlinuz` with `initrd.img` and nothing
  else, with no menu shown, and sets the kernel's command line to the fixed
  line of `stick/cmdline`. `initrd.img` holds only the program of SK-10, the
  kernel modules of SK-04 and the directories they need.
- **SK-04. The modules.** The only modules in `initrd.img` are those named
  in `stick/modules.list`: the USB host and keyboard drivers, and the
  display drivers a PC needs to show the program, each a file of the same
  kernel package, unchanged and signed by the distribution. No network,
  wireless, Bluetooth, storage, sound, camera or file system module is
  listed, except the camera modules of SK-51, which are loaded only in the
  mode of section 8. A test reads both lists and refuses any module of the
  first under the kernel's
  `drivers/net`, `net`, `drivers/bluetooth`, `drivers/usb/storage`,
  `drivers/scsi`, `drivers/ata`, `drivers/nvme`, `fs`, `sound` or
  `drivers/media`, and any module of the camera list outside
  `drivers/media/usb/uvc`, `drivers/media/common/videobuf2`,
  `drivers/media/v4l2-core` and `drivers/media/mc`, except the one file
  `drivers/media/common/uvc` that the USB video class driver needs. The
  camera modules sit apart from the others in `initrd.img`, so that start-up
  does not load them.
- **SK-05. Same input, same bytes.** Two builds of one commit with the same
  `inputs.lock` give byte-identical files: the archive's members sorted,
  every time and owner fixed, compressed with no name or time.
  `SHA256SUMS` lists every other file of the folder.

## 2. Starting

- **SK-06. Secure Boot stays on.** The stick needs no firmware setting to
  change and no key to enroll. On a PC whose firmware trusts Microsoft's
  third-party UEFI CA, the chain is the firmware, the signed shim, the
  signed GRUB and the signed kernel; the kernel then runs under lockdown.
  The program reads the firmware's `SecureBoot` value and the kernel's
  lockdown mode and shows both on its first screen.
- **SK-07. What it does not cover.** The initramfs and `grub.cfg` are not
  signed: a changed stick still starts. The owner checks the folder against
  the published `SHA256SUMS` before the first start (SK-30). Apple Silicon
  Macs, Intel Macs with the T2 chip at their default setting, and PCs that
  ship with the third-party CA turned off do not start it; the handbook says
  so plainly.

## 3. The program

- **SK-10. One program.** The kernel runs one static program as process 1,
  built from this repository (`crates/openbmt-stick`). There is no shell, no
  other program, no login and no way to start one.
- **SK-11. What it mounts.** It mounts `proc`, `sysfs`, `devtmpfs`,
  `efivarfs` and `securityfs` (the last two read only; `securityfs` is the
  only place the kernel shows its lockdown mode) and nothing else. It never mounts a disk, the stick
  included: the program runs from memory.
- **SK-12. No network, checked.** It never brings an interface up, never
  sets an address and never opens a socket. On the first
  screen it lists every interface the kernel has; with a built-in driver an
  interface may exist, and it stays down. A test reads the program's source
  and refuses any socket call.
- **SK-13. Nothing written.** It writes to no disk, no firmware variable and
  no file outside memory. When the owner is done it turns the computer off,
  and the words are gone with the memory.
- **SK-14. The screen and the keys.** It draws on the frame buffer the
  firmware or the kernel's display driver gives, in large text, and reads
  the keyboard through the kernel's input events. No mouse is needed.
- **SK-15. The words of every screen** are the constants of
  `crates/openbmt-stick/src/words.rs`, each distinct and each shown by some
  screen. No other sentence is made by the program: a screen holds only those
  words, the words of the list, numbers, hex and key names. The firmware's
  Secure Boot value is shown as a word (on, off, unknown), never as a number.

## 4. Chance

- **SK-20. Two sources, either enough.** A key is made from the kernel's
  random bytes and, if the owner wants, their own die rolls. The program
  asks the kernel for 32 bytes with `getrandom` and waits until the kernel
  says its generator is ready. The 16 bytes of entropy are the first 16
  bytes of SHA-256 over the tag `OpenBMT/stick/v1`, a zero byte, the 32
  kernel bytes, and the die rolls as ASCII digits. Either source alone,
  honest, makes the key unguessable.
- **SK-21. Die rolls with the computer's.** Any number of rolls from 0 is
  taken, each 1 to 6; the screen says that 50 rolls are enough on their own.
  Undo removes the last roll.
- **SK-22. Dice alone.** The owner may choose dice alone: rolls taken in
  threes until 50 values are kept, as `docs/KEYMAKER-RULES.md` KM-30 says
  (about 270 rolls, since a three with a number repeated is dropped), then
  turned into words as KM-31 says, with no kernel bytes. The screen shows the
  rolls made and the values kept out of 50; a roll after the 50th value is
  refused. The same rolls give the same words as the Key Maker page
  and as the hand method of that page.
- **SK-23. What is fixed.** 12 English BIP39 words, no passphrase, native
  SegWit with a single key (BIP84), account `m/84'/0'/0'`, real bitcoin
  only, as KM-01.

## 5. What comes out

- **SK-25. The words.** The 12 words are shown four at a time, numbered,
  for the owner to write by hand. Then three positions among the 12 are asked
  back. They are picked from 32 fresh kernel bytes by KM-16's byte rule (each
  byte below 252, modulo 12, skipping repeats), also in a dice-alone run,
  where kernel bytes pick only the positions and never touch the key. The
  answers are checked as KM-16 says. A wrong answer
  shows the words again.
- **SK-26. The public result.** Then the program shows the master
  fingerprint, the first receive address and the wallet's descriptor
  `wpkh([fp/84h/0h/0h]xpub/<0;1>/*)#checksum`, as text and as a QR code the
  owner phone's "Connect your wallet" scans (`docs/OWNER-APP-RULES.md`).
  The descriptor's QR code has a screen of its own. The
  outputs equal KM-06 for the same words.
- **SK-27. SeedQR, if asked.** The owner may ask for the words as a Compact
  SeedQR (the SeedSigner standard) to copy onto steel. The screen says that
  the code is the key itself and must never be photographed.
- **SK-28. Check my words.** The owner may type 12 words instead; the
  program checks the BIP39 checksum and shows SK-26 for them, so a written
  card can be checked against the wallet without any other computer.
- **SK-29. Turn off.** Every screen after the words has "Turn off". It
  clears the program's memory of the words and powers the computer off.

## 5a. Keys for the shop's everyday wallets

- **SK-32. Made from the same words.** On the public result screen (SK-26),
  after a new key or after "Check my words" (SK-28), the owner may ask for
  the key of one of the shop's two small everyday wallets. Both are BIP-85
  children of the 12 words, so there is nothing new to write down:
  - **The refund float:** the BIP-85 BIP39 child of 12 English words at
    `m/83696968'/39'/0'/12'/<index>'`, shown as a Compact SeedQR (SK-27).
    The index starts at 0 and the owner may change it from 0 to 99 on the
    screen, which shows it; a new float after a lost phone uses the next
    index (`docs/FLOAT-RULES.md` FL-04).
  - **Shop Lightning:** the BIP-85 HEX child of 64 bytes at
    `m/83696968'/128169'/64'/0'` (`docs/SHOP-LIGHTNING-RULES.md` SL-10), shown
    as a QR code of its 128 hexadecimal characters in lower case.
- **SK-33. Said on the screen.** Each of the two screens says what the code
  is, that it can spend that wallet's money and nothing else, that it is
  scanned once by the owner phone or the Lightning counter, and that it is
  never photographed.
- **SK-34. Tested.** Both children equal the BIP-85 specification's test
  vectors for the specification's own key, and the stick's QR codes decode
  to the same bytes. The node's ID of SK-35 equals the core's for the same
  child.
- **SK-35. The Lightning node's ID.** After the Shop Lightning code, the
  stick shows the node's ID made from it (`docs/SHOP-LIGHTNING-RULES.md`
  SL-72), as a QR code of its 66 hexadecimal characters in lower case, with
  its first and last 8 characters in large type. The screen says that the
  ID is public and spends nothing, and that the owner phone scans it to turn
  Shop Lightning on. The ID is made by `openbmt-core`, as the counter makes
  it.

## 6. Checking the stick

- **SK-30. Before the first start.** The release publishes `SHA256SUMS`
  signed with the project's release key, and the handbook gives the steps
  to check every file on an ordinary computer before the first start.
- **SK-31. On the screen.** The first screen shows the program's own
  version and the SHA-256 of `initrd.img` as the kernel loaded it, read from
  memory, for the owner to compare with the release page.

## 8. Sign a payment

- **SK-50. A fourth choice.** The first screen has "Sign a payment". It is
  for the rare payment from savings, prepared by the owner phone's "Send
  from your wallet" (`docs/OWNER-APP-RULES.md` OA-115 to OA-118). Nothing of
  this mode runs unless the owner picks it.
- **SK-51. The camera, only here.** Picking it loads the camera modules
  named in `stick/modules-camera.list`: the USB video class driver and the
  modules it needs, each a file of the same kernel package, unchanged and
  signed. They are in `initrd.img` but never loaded otherwise. Still no
  network, no disk and nothing written (SK-11 to SK-13).
- **SK-52. Reading the payment.** The program reads frames from the first
  camera as grey levels and decodes QR codes from them. The payment is a
  PSBT (BIP 174, version 0) sent as an animated UR `crypto-psbt` or as BBQr,
  the two forms the owner phone shows (`docs/UR-RULES.md`,
  `docs/BBQR-RULES.md`), decoded by the repository's own code. The screen
  shows the parts read out of the parts needed. No camera after a set time:
  the screen says so and the owner can turn off.
- **SK-53. The words.** Then the owner types the 12 words, as SK-28. A
  payment whose inputs are not of the wallet of these words is refused
  before anything is shown (SK-54).
- **SK-54. What is refused.** The payment is refused, naming why, when: an
  input lacks its previous output or its key path; an input's key path is
  not `m/84'/0'/0'/0/i` or `m/84'/0'/0'/1/i` under the master fingerprint of
  these words, or the key at that path is not the input's key; an input asks
  for a signature other than SIGHASH_ALL; the payment is not for real
  bitcoin; the outputs pay more than the inputs hold; the file is over
  8 MiB or has bytes after its last map, as the signer refuses
  (`docs/SIGNER-RULES.md`).
- **SK-55. What is shown before signing.** Every output, with its address
  and amount in bitcoin. An output is marked as the wallet's own only when
  the program derives its address itself from these words; a claim in the
  PSBT is not enough. Then the total sent away, the fee and the fee per
  virtual byte. Enter signs; Esc refuses and nothing is signed.
- **SK-56. What comes out.** The PSBT with this wallet's signatures added to
  every input, not finalized, shown as an animated QR code in the form it
  came in, for the owner phone to scan back. Nothing else of the PSBT is
  changed.
- **SK-57. Afterwards.** "Turn off" clears the words, the payment and the
  frames, as SK-29.
- **SK-58. Tested.** Unit tests: refusals of SK-54 one by one; own outputs
  marked only when derived; the signatures are valid for the inputs and the
  PSBT otherwise unchanged; decoding of UR and BBQr from frames that are
  scaled, rotated, blurred and noisy grey images of the owner phone's codes.
  In QEMU: the camera modules load only after the choice, and a whole run
  signs a payment fed through a test-only feature `serial-frames` that
  reads frames from the serial port instead of a camera, and that, like
  `serial-echo`, no release contains. The signed PSBT is checked against
  `crates/openbmt-owner`'s own-spend finishing. A real camera on a real PC
  is tested by hand.

## 7. Tests

- **SK-40. In a virtual machine.** `stick/test-qemu.sh` builds the folder
  and starts it in QEMU with OVMF in Secure Boot mode with Microsoft's keys
  enrolled and no network device, drives a whole run with QEMU's key input,
  and checks the public result against the Rust core for the same rolls. A
  build feature `serial-echo`, used only by this test and never in a
  release, copies every screen's text to the serial port; the release build
  is checked not to contain it.
- **SK-41. Refused when unsigned.** The same machine refuses a copy of the
  folder whose `BOOTX64.EFI` has its signature removed.
- **SK-42. The core.** Mixing (SK-20), dice alone (SK-22, equal to the Key
  Maker page's vectors), the outputs (SK-26, equal to KM-06), SeedQR (SK-27,
  against the SeedSigner standard's published examples) and the check of
  typed words (SK-28) are unit tests of `crates/openbmt-stick` that run on
  any computer.
