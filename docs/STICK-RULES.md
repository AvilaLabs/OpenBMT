# Stick rules: the shop's key made on an ordinary computer, offline

Status: specification, written before the code. Each rule has an ID so that
a test can name the rule it proves (`sk_NN_...`).

The stick is a folder the owner copies onto any USB stick. An ordinary PC
started from it runs one program and nothing else: the key maker. The
computer's own system never starts, no network is ever turned on, and
nothing is written anywhere. The owner leaves with the wallet's words in
their own hand and the wallet's public description on the owner phone.

The stick makes the key. It does not sign: a shop signs with a hardware
signer, or pays its refunds from a small capped wallet of its own.

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
  listed. A test reads the list and refuses any module under the kernel's
  `drivers/net`, `net`, `drivers/bluetooth`, `drivers/usb/storage`,
  `drivers/scsi`, `drivers/ata`, `drivers/nvme`, `fs`, `sound` or
  `drivers/media`.
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
- **SK-11. What it mounts.** It mounts `proc`, `sysfs`, `devtmpfs` and
  `efivarfs` (read only) and nothing else. It never mounts a disk, the stick
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
- **SK-15. The words of every screen** are under `docs/SCREEN-RULES.md`;
  until they are written a placeholder constant `WORDS_NEEDED_SK_xxx` names
  the rule.

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
- **SK-22. Dice alone.** The owner may choose dice alone: exactly 50 rolls,
  turned into words as `docs/KEYMAKER-RULES.md` KM-30 and KM-31 say, with
  no kernel bytes. The same rolls give the same words as the Key Maker page
  and as the hand method of that page.
- **SK-23. What is fixed.** 12 English BIP39 words, no passphrase, native
  SegWit with a single key (BIP84), account `m/84'/0'/0'`, real bitcoin
  only, as KM-01.

## 5. What comes out

- **SK-25. The words.** The 12 words are shown four at a time, numbered,
  for the owner to write by hand. Then three positions among the 12, picked
  from the kernel's random bytes, are asked back, as KM-16. A wrong answer
  shows the words again.
- **SK-26. The public result.** Then the program shows the master
  fingerprint, the first receive address and the wallet's descriptor
  `wpkh([fp/84h/0h/0h]xpub/<0;1>/*)#checksum`, as text and as a QR code the
  owner phone's "Connect your wallet" scans (`docs/OWNER-APP-RULES.md`). The
  outputs equal KM-06 for the same words.
- **SK-27. SeedQR, if asked.** The owner may ask for the words as a Compact
  SeedQR (the SeedSigner standard) to copy onto steel. The screen says that
  the code is the key itself and must never be photographed.
- **SK-28. Check my words.** The owner may type 12 words instead; the
  program checks the BIP39 checksum and shows SK-26 for them, so a written
  card can be checked against the wallet without any other computer.
- **SK-29. Turn off.** Every screen after the words has "Turn off". It
  clears the program's memory of the words and powers the computer off.

## 6. Checking the stick

- **SK-30. Before the first start.** The release publishes `SHA256SUMS`
  signed with the project's release key, and the handbook gives the steps
  to check every file on an ordinary computer before the first start.
- **SK-31. On the screen.** The first screen shows the program's own
  version and the SHA-256 of `initrd.img` as the kernel loaded it, read from
  memory, for the owner to compare with the release page.

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
