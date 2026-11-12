// Published vectors: BIP39 (trezor python-mnemonic), BIP32 1 and 2, BIP84,
// BIP380. Sources and commits are in test/vectors/SOURCES.md.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { sha256 } from '@noble/hashes/sha2.js';
import { bytesToHex, hexToBytes } from '@noble/hashes/utils.js';
import { createBase58check } from '@scure/base';
import { HDKey } from '@scure/bip32';
import { entropyToMnemonic, mnemonicToSeedSync } from '@scure/bip39';
import { wordlist } from '@scure/bip39/wordlists/english.js';
import {
  checkDescriptor,
  descriptorChecksum,
  entropyToWords,
  withChecksum,
  wordsToWallet,
} from '../src/core.js';

const here = new URL('.', import.meta.url);
const bip39 = JSON.parse(readFileSync(new URL('vectors/bip39-english.json', here), 'utf8')).english;

test('km_11 BIP39 English vectors: entropy, words, seed, root key', () => {
  assert.equal(bip39.length, 24);
  for (const [entropy, words, seed, xprv] of bip39) {
    const bytes = hexToBytes(entropy);
    assert.equal(entropyToMnemonic(bytes, wordlist), words);
    if (bytes.length === 16) assert.equal(entropyToWords(bytes), words);
    const s = mnemonicToSeedSync(words, 'TREZOR');
    assert.equal(bytesToHex(s), seed);
    assert.equal(HDKey.fromMasterSeed(s).privateExtendedKey, xprv);
  }
});

const bip32 = [
  {
    seed: '000102030405060708090a0b0c0d0e0f',
    chains: [
      ['m', 'xpub661MyMwAqRbcFtXgS5sYJABqqG9YLmC4Q1Rdap9gSE8NqtwybGhePY2gZ29ESFjqJoCu1Rupje8YtGqsefD265TMg7usUDFdp6W1EGMcet8', 'xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi'],
      ["m/0'", 'xpub68Gmy5EdvgibQVfPdqkBBCHxA5htiqg55crXYuXoQRKfDBFA1WEjWgP6LHhwBZeNK1VTsfTFUHCdrfp1bgwQ9xv5ski8PX9rL2dZXvgGDnw', 'xprv9uHRZZhk6KAJC1avXpDAp4MDc3sQKNxDiPvvkX8Br5ngLNv1TxvUxt4cV1rGL5hj6KCesnDYUhd7oWgT11eZG7XnxHrnYeSvkzY7d2bhkJ7'],
      ["m/0'/1", 'xpub6ASuArnXKPbfEwhqN6e3mwBcDTgzisQN1wXN9BJcM47sSikHjJf3UFHKkNAWbWMiGj7Wf5uMash7SyYq527Hqck2AxYysAA7xmALppuCkwQ', 'xprv9wTYmMFdV23N2TdNG573QoEsfRrWKQgWeibmLntzniatZvR9BmLnvSxqu53Kw1UmYPxLgboyZQaXwTCg8MSY3H2EU4pWcQDnRnrVA1xe8fs'],
      ["m/0'/1/2'", 'xpub6D4BDPcP2GT577Vvch3R8wDkScZWzQzMMUm3PWbmWvVJrZwQY4VUNgqFJPMM3No2dFDFGTsxxpG5uJh7n7epu4trkrX7x7DogT5Uv6fcLW5', 'xprv9z4pot5VBttmtdRTWfWQmoH1taj2axGVzFqSb8C9xaxKymcFzXBDptWmT7FwuEzG3ryjH4ktypQSAewRiNMjANTtpgP4mLTj34bhnZX7UiM'],
      ["m/0'/1/2'/2", 'xpub6FHa3pjLCk84BayeJxFW2SP4XRrFd1JYnxeLeU8EqN3vDfZmbqBqaGJAyiLjTAwm6ZLRQUMv1ZACTj37sR62cfN7fe5JnJ7dh8zL4fiyLHV', 'xprvA2JDeKCSNNZky6uBCviVfJSKyQ1mDYahRjijr5idH2WwLsEd4Hsb2Tyh8RfQMuPh7f7RtyzTtdrbdqqsunu5Mm3wDvUAKRHSC34sJ7in334'],
      ["m/0'/1/2'/2/1000000000", 'xpub6H1LXWLaKsWFhvm6RVpEL9P4KfRZSW7abD2ttkWP3SSQvnyA8FSVqNTEcYFgJS2UaFcxupHiYkro49S8yGasTvXEYBVPamhGW6cFJodrTHy', 'xprvA41z7zogVVwxVSgdKUHDy1SKmdb533PjDz7J6N6mV6uS3ze1ai8FHa8kmHScGpWmj4WggLyQjgPie1rFSruoUihUZREPSL39UNdE3BBDu76'],
    ],
  },
  {
    seed: 'fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a29f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b484542',
    chains: [
      ['m', 'xpub661MyMwAqRbcFW31YEwpkMuc5THy2PSt5bDMsktWQcFF8syAmRUapSCGu8ED9W6oDMSgv6Zz8idoc4a6mr8BDzTJY47LJhkJ8UB7WEGuduB', 'xprv9s21ZrQH143K31xYSDQpPDxsXRTUcvj2iNHm5NUtrGiGG5e2DtALGdso3pGz6ssrdK4PFmM8NSpSBHNqPqm55Qn3LqFtT2emdEXVYsCzC2U'],
      ['m/0', 'xpub69H7F5d8KSRgmmdJg2KhpAK8SR3DjMwAdkxj3ZuxV27CprR9LgpeyGmXUbC6wb7ERfvrnKZjXoUmmDznezpbZb7ap6r1D3tgFxHmwMkQTPH', 'xprv9vHkqa6EV4sPZHYqZznhT2NPtPCjKuDKGY38FBWLvgaDx45zo9WQRUT3dKYnjwih2yJD9mkrocEZXo1ex8G81dwSM1fwqWpWkeS3v86pgKt'],
      ["m/0/2147483647'", 'xpub6ASAVgeehLbnwdqV6UKMHVzgqAG8Gr6riv3Fxxpj8ksbH9ebxaEyBLZ85ySDhKiLDBrQSARLq1uNRts8RuJiHjaDMBU4Zn9h8LZNnBC5y4a', 'xprv9wSp6B7kry3Vj9m1zSnLvN3xH8RdsPP1Mh7fAaR7aRLcQMKTR2vidYEeEg2mUCTAwCd6vnxVrcjfy2kRgVsFawNzmjuHc2YmYRmagcEPdU9'],
      ["m/0/2147483647'/1", 'xpub6DF8uhdarytz3FWdA8TvFSvvAh8dP3283MY7p2V4SeE2wyWmG5mg5EwVvmdMVCQcoNJxGoWaU9DCWh89LojfZ537wTfunKau47EL2dhHKon', 'xprv9zFnWC6h2cLgpmSA46vutJzBcfJ8yaJGg8cX1e5StJh45BBciYTRXSd25UEPVuesF9yog62tGAQtHjXajPPdbRCHuWS6T8XA2ECKADdw4Ef'],
      ["m/0/2147483647'/1/2147483646'", 'xpub6ERApfZwUNrhLCkDtcHTcxd75RbzS1ed54G1LkBUHQVHQKqhMkhgbmJbZRkrgZw4koxb5JaHWkY4ALHY2grBGRjaDMzQLcgJvLJuZZvRcEL', 'xprvA1RpRA33e1JQ7ifknakTFpgNXPmW2YvmhqLQYMmrj4xJXXWYpDPS3xz7iAxn8L39njGVyuoseXzU6rcxFLJ8HFsTjSyQbLYnMpCqE2VbFWc'],
      ["m/0/2147483647'/1/2147483646'/2", 'xpub6FnCn6nSzZAw5Tw7cgR9bi15UV96gLZhjDstkXXxvCLsUXBGXPdSnLFbdpq8p9HmGsApME5hQTZ3emM2rnY5agb9rXpVGyy3bdW6EEgAtqt', 'xprvA2nrNbFZABcdryreWet9Ea4LvTJcGsqrMzxHx98MMrotbir7yrKCEXw7nadnHM8Dq38EGfSh6dqA9QWTyefMLEcBYJUuekgW4BYPJcr9E7j'],
    ],
  },
];

test('km_12 BIP32 test vectors 1 and 2', () => {
  for (const { seed, chains } of bip32) {
    const root = HDKey.fromMasterSeed(hexToBytes(seed));
    for (const [path, xpub, xprv] of chains) {
      const node = root.derive(path);
      assert.equal(node.publicExtendedKey, xpub, path);
      assert.equal(node.privateExtendedKey, xprv, path);
    }
  }
});

const b58 = createBase58check(sha256);

// BIP84 publishes the account key as a zpub; the descriptor needs a plain xpub.
function zpubToXpub(z) {
  const raw = b58.decode(z);
  assert.deepEqual([...raw.slice(0, 4)], [0x04, 0xb2, 0x47, 0x46]);
  const out = new Uint8Array(raw);
  out.set([0x04, 0x88, 0xb2, 0x1e], 0);
  return b58.encode(out);
}

test('km_13 BIP84 vectors: account xpub and first receive address', () => {
  const words = 'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';
  const w = wordsToWallet(words);
  const zpub = 'zpub6rFR7y4Q2AijBEqTUquhVz398htDFrtymD9xYYfG1m4wAcvPhXNfE3EfH1r1ADqtfSdVCToUG868RvUUkgDKf31mGDtKsAYz2oz2AGutZYs';
  assert.equal(w.xpub, zpubToXpub(zpub));
  assert.equal(w.address, 'bc1qcr8te4kr609gcawutmrza0j4xv80jy8z306fyu');
  const root = HDKey.fromMasterSeed(mnemonicToSeedSync(words, ''));
  assert.equal(
    bytesToHex(root.derive("m/84'/0'/0'/0/0").publicKey),
    '0330d54fd0dd420a6e5f8d3624f5f3482cae350f79d5f0753bf5beef9c2d91af3c',
  );
  const zroot = 'zpub6jftahH18ngZxLmXaKw3GSZzZsszmt9WqedkyZdezFtWRFBZqsQH5hyUmb4pCEeZGmVfQuP5bedXTB8is6fTv19U1GQRyQUKQGUTzyHACMF';
  assert.equal(root.publicExtendedKey, zpubToXpub(zroot));
});

test('km_10 BIP380 checksum vectors', () => {
  assert.equal(descriptorChecksum('raw(deadbeef)'), '89f8spxm');
  assert.equal(withChecksum('raw(deadbeef)'), 'raw(deadbeef)#89f8spxm');
  assert.equal(checkDescriptor('raw(deadbeef)#89f8spxm'), true);
  for (const bad of [
    'raw(deadbeef)',
    'raw(deadbeef)#',
    'raw(deadbeef)#89f8spxmx',
    'raw(deadbeef)#89f8spx',
    'raw(deedbeef)#89f8spxm',
    'raw(deedbeef)##9f8spxm',
    'raw(Ü)#00000000',
  ]) {
    assert.equal(checkDescriptor(bad), false, bad);
  }
});
