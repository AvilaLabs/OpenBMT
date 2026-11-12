# Runs inside the Python that ships with Electrum 4.8.4 (see crosscheck.sh).
# Reads flip strings, one per line, and prints for each:
#   flips words fingerprint xpub first-receive-address
# using Electrum's own word list, BIP39 seed and BIP32 code.
import hashlib
import sys

from electrum import bip32, bitcoin, keystore
from electrum.mnemonic import Wordlist


def words_of(flips):
    wl = Wordlist.from_file("english.txt")
    entropy = int(flips, 2).to_bytes(16, "big")
    check = hashlib.sha256(entropy).digest()[0] >> 4
    bits = flips + format(check, "04b")
    return " ".join(wl[int(bits[i : i + 11], 2)] for i in range(0, 132, 11))


for line in sys.stdin.read().split():
    words = words_of(line)
    seed = keystore.bip39_to_seed(words, passphrase="")
    root = bip32.BIP32Node.from_rootseed(seed, xtype="standard")
    account = root.subkey_at_private_derivation("m/84'/0'/0'")
    first = root.subkey_at_private_derivation("m/84'/0'/0'/0/0")
    address = bitcoin.pubkey_to_address("p2wpkh", first.eckey.get_public_key_hex(compressed=True))
    print(line, words, root.calc_fingerprint_of_this_node().hex(), account.to_xpub(), address)
