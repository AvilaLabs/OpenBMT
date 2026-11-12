// The key maker's core: coin flips to a wallet, with nothing else in between.
// No screens, no storage, no network, and no source of chance: the only bits
// that enter are the flips the owner passes in.

import { ripemd160 } from '@noble/hashes/legacy.js';
import { sha256 } from '@noble/hashes/sha2.js';
import { bytesToHex } from '@noble/hashes/utils.js';
import { bech32 } from '@scure/base';
import { HDKey } from '@scure/bip32';
import { entropyToMnemonic, mnemonicToSeedSync } from '@scure/bip39';
import { wordlist } from '@scure/bip39/wordlists/english.js';

export const FLIP_COUNT = 128;
export const ACCOUNT_PATH = "m/84'/0'/0'";
export const FIRST_RECEIVE_PATH = "m/84'/0'/0'/0/0";

// BIP380, copied from the specification text.
const INPUT_CHARSET =
  '0123456789()[],\'/*abcdefgh@:$%{}IJKLMNOPQRSTUVWXYZ&+-.;<=>?!^_|~ijklmnopqrstuvwxyzABCDEFGH`#"\\ ';
const CHECKSUM_CHARSET = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';
const GENERATOR = [
  0xf5dee51989n,
  0xa9fdca3312n,
  0x1bab10e32dn,
  0x3706b1677an,
  0x644d626ffdn,
];

function polymod(symbols) {
  let chk = 1n;
  for (const value of symbols) {
    const top = chk >> 35n;
    chk = ((chk & 0x7ffffffffn) << 5n) ^ BigInt(value);
    for (let i = 0n; i < 5n; i++) {
      if ((top >> i) & 1n) chk ^= GENERATOR[Number(i)];
    }
  }
  return chk;
}

function expand(s) {
  const groups = [];
  const symbols = [];
  for (const c of s) {
    const v = INPUT_CHARSET.indexOf(c);
    if (v < 0) throw new Error('KM-09: character outside the descriptor set');
    symbols.push(v & 31);
    groups.push(v >> 5);
    if (groups.length === 3) {
      symbols.push(groups[0] * 9 + groups[1] * 3 + groups[2]);
      groups.length = 0;
    }
  }
  if (groups.length === 1) symbols.push(groups[0]);
  else if (groups.length === 2) symbols.push(groups[0] * 3 + groups[1]);
  return symbols;
}

/** The 8-character BIP380 checksum of a descriptor string (no `#`). */
export function descriptorChecksum(s) {
  const symbols = expand(s).concat([0, 0, 0, 0, 0, 0, 0, 0]);
  const chk = polymod(symbols) ^ 1n;
  let out = '';
  for (let i = 0n; i < 8n; i++) {
    out += CHECKSUM_CHARSET[Number((chk >> (5n * (7n - i))) & 31n)];
  }
  return out;
}

/** True when `s` is `SCRIPT#CHECKSUM` with a correct checksum (BIP380). */
export function checkDescriptor(s) {
  if (s.length < 9 || s[s.length - 9] !== '#') return false;
  const tail = s.slice(-8);
  for (const c of tail) if (!CHECKSUM_CHARSET.includes(c)) return false;
  const body = s.slice(0, -9);
  for (const c of body) if (!INPUT_CHARSET.includes(c)) return false;
  const symbols = expand(body).concat([...tail].map((c) => CHECKSUM_CHARSET.indexOf(c)));
  return polymod(symbols) === 1n;
}

/** The descriptor with its checksum appended. */
export function withChecksum(s) {
  return `${s}#${descriptorChecksum(s)}`;
}

/**
 * Exactly 128 flips, heads = 1, tails = 0, first flip the most significant
 * bit. Anything else is refused. Returns 16 bytes.
 */
export function flipsToEntropy(flips) {
  if (!Array.isArray(flips) || flips.length !== FLIP_COUNT) {
    throw new Error(`KM-04: exactly ${FLIP_COUNT} flips are needed`);
  }
  const out = new Uint8Array(FLIP_COUNT / 8);
  for (let i = 0; i < FLIP_COUNT; i++) {
    const f = flips[i];
    if (f !== 0 && f !== 1) {
      throw new Error('KM-04: a flip must be 1 (heads) or 0 (tails)');
    }
    out[i >> 3] |= f << (7 - (i & 7));
  }
  return out;
}

/** BIP39 English words for 16 bytes of entropy: 12 words, space separated. */
export function entropyToWords(entropy) {
  if (!(entropy instanceof Uint8Array) || entropy.length !== 16) {
    throw new Error('KM-04: 16 bytes of entropy are needed');
  }
  return entropyToMnemonic(entropy, wordlist);
}

function hex8(n) {
  return n.toString(16).padStart(8, '0');
}

/** Wallet outputs for 12 words, no passphrase. */
export function wordsToWallet(words) {
  const seed = mnemonicToSeedSync(words, '');
  const root = HDKey.fromMasterSeed(seed);
  const fingerprint = hex8(root.fingerprint);
  const account = root.derive(ACCOUNT_PATH);
  const xpub = account.publicExtendedKey;
  const first = root.derive(FIRST_RECEIVE_PATH);
  const h160 = ripemd160(sha256(first.publicKey));
  const address = bech32.encode('bc', [0, ...bech32.toWords(h160)]);
  const origin = `[${fingerprint}/84h/0h/0h]${xpub}`;
  return {
    fingerprint,
    xpub,
    address,
    descriptor: withChecksum(`wpkh(${origin}/<0;1>/*)`),
    receiveDescriptor: withChecksum(`wpkh(${origin}/0/*)`),
    changeDescriptor: withChecksum(`wpkh(${origin}/1/*)`),
  };
}

/**
 * The working for "Check it by hand": eleven 11-bit groups, the last 7 bits
 * with the 4 check bits, each with its number and word.
 */
export function arithmetic(flips) {
  const entropy = flipsToEntropy(flips);
  const words = entropyToWords(entropy).split(' ');
  const check = sha256(entropy)[0] >> 4;
  const groups = [];
  for (let g = 0; g < 11; g++) {
    const bits = flips.slice(g * 11, g * 11 + 11);
    const number = parseInt(bits.join(''), 2);
    groups.push({ bits: bits.join(''), number, word: wordlist[number] });
    if (wordlist[number] !== words[g]) {
      throw new Error('KM-07: working does not match the words');
    }
  }
  const lastBits = flips.slice(121).join('');
  const checkBits = check.toString(2).padStart(4, '0');
  const number = parseInt(lastBits + checkBits, 2);
  if (wordlist[number] !== words[11]) {
    throw new Error('KM-07: working does not match the words');
  }
  return {
    groups,
    last: { bits: lastBits, checkBits, number, word: wordlist[number] },
    entropyHex: bytesToHex(entropy),
  };
}

/** Everything from flips: words, wallet and working. */
export function fromFlips(flips) {
  const words = entropyToWords(flipsToEntropy(flips));
  return { words, wallet: wordsToWallet(words), working: arithmetic(flips) };
}
