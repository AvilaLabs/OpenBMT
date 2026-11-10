#![no_main]

use bip324::{
    CipherSession, FillBytes, GarbageResult, Handshake, PacketType, ReceivedKey, Role,
    VersionResult,
};
use bitcoin::consensus::encode::serialize;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::{Address, ServiceFlags};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_net::{Session, SessionConfig, V2Entropy, MAX_OUTGOING_BYTES};

struct Fixed {
    values: [[u8; 32]; 2],
    drawn: usize,
}

impl FillBytes for Fixed {
    fn fill_bytes(&mut self, dest: &mut [u8; 32]) {
        *dest = self.values[self.drawn.min(1)];
        self.drawn += 1;
    }
}

fn packet(cipher: &mut CipherSession, contents: &[u8], decoy: bool) -> Vec<u8> {
    let kind = if decoy {
        PacketType::Decoy
    } else {
        PacketType::Genuine
    };
    let mut out = vec![0u8; contents.len() + 20];
    // Contents that cannot be encrypted leave nothing to feed.
    if cipher
        .outbound()
        .encrypt(contents, &mut out, kind, None)
        .is_err()
    {
        return Vec::new();
    }
    out
}

/// Contents of a message with a long-form command.
fn long(name: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 13];
    out[1..1 + name.len()].copy_from_slice(name);
    out.extend_from_slice(payload);
    out
}

/// A session on the encrypted transport whose handshake with a fixed
/// responder is through, and the responder's cipher.
fn through_handshake() -> Option<(Session, CipherSession)> {
    let config = SessionConfig {
        network: Network::Regtest,
        nonce: 0x0102_0304_0506_0708,
        tip_height: 100,
    };
    let mut session = Session::new_v2(config, 1_000, V2Entropy::new([7; 32], [8; 32])).ok()?;
    let ours = session.take_outgoing();
    let their: [u8; 64] = ours.get(..64)?.try_into().ok()?;
    let mut feed = Fixed {
        values: [[3; 32], [4; 32]],
        drawn: 0,
    };
    let curve = Secp256k1::new();
    let magic = Network::Regtest.magic().to_bytes();
    let h = Handshake::new_with_rng(magic, Role::Responder, &mut feed, &curve).ok()?;
    let mut first = vec![0u8; 64];
    let h = h.send_key(None, &mut first).ok()?;
    let h = h.receive_key(their).ok()?;
    let mut tail = vec![0u8; Handshake::<ReceivedKey<'_>>::send_version_len(None)];
    let h = h.send_version(&mut tail, None).ok()?;
    first.extend_from_slice(&tail);
    session.receive(&first, 1_001).ok()?;
    let ours = session.take_outgoing();
    let GarbageResult::FoundGarbage {
        mut handshake,
        consumed_bytes,
    } = h.receive_garbage(&ours).ok()?
    else {
        return None;
    };
    let head: [u8; 3] = ours
        .get(consumed_bytes..consumed_bytes + 3)?
        .try_into()
        .ok()?;
    let rest = handshake.decrypt_packet_len(head).ok()?;
    let mut body = ours
        .get(consumed_bytes + 3..consumed_bytes + 3 + rest)?
        .to_vec();
    let VersionResult::Complete { mut cipher } = handshake.receive_version(&mut body).ok()? else {
        return None;
    };
    let silent = Address {
        services: ServiceFlags::NONE,
        address: [0; 8],
        port: 0,
    };
    let version = VersionMessage {
        version: 70_016,
        services: ServiceFlags::NETWORK | ServiceFlags::WITNESS | ServiceFlags::P2P_V2,
        timestamp: 1_000,
        receiver: silent.clone(),
        sender: silent,
        nonce: 42,
        user_agent: "/fuzz/".to_string(),
        start_height: 100,
        relay: true,
    };
    let payload = serialize(&NetworkMessage::Version(version));
    let mut flight = packet(&mut cipher, &long(b"version", &payload), false);
    flight.extend(packet(&mut cipher, &long(b"verack", &[]), false));
    session.receive(&flight, 1_002).ok()?;
    let _ = session.take_outgoing();
    Some((session, cipher))
}

fuzz_target!(|data: &[u8]| {
    // The first byte is the size of each read, the second says whether the
    // rest is the raw stream (bit 0 clear) or the contents of valid packets,
    // each chunk one packet (bit 0 set; bit 1 marks them as decoys).
    let [size, mode, stream @ ..] = data else {
        return;
    };
    let Some((mut session, mut cipher)) = through_handshake() else {
        return;
    };
    let step = usize::from(*size) * 17 + 1;
    let mut now = 1_003u64;
    let mut wire = Vec::new();
    if mode & 1 == 0 {
        wire.extend_from_slice(stream);
    } else {
        for chunk in stream.chunks(step) {
            wire.extend(packet(&mut cipher, chunk, mode & 2 != 0));
        }
    }
    for chunk in wire.chunks(step) {
        now += 1;
        // It must never panic. A drop names a rule and ends the session.
        match session.receive(chunk, now) {
            Ok(_) => {}
            Err(drop) => {
                assert!(drop.to_string().starts_with(drop.rule()));
                assert_eq!(session.tick(now), Err(drop));
                return;
            }
        }
        assert!(session.take_outgoing().len() <= MAX_OUTGOING_BYTES);
    }
});
