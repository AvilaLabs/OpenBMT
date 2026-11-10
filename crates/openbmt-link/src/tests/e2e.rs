//! LK-09: the link gives no authority. The real types that will travel.

use openbmt_core::batch::PinnedRoots;
use openbmt_core::command::{Action, CommandError, OwnerCommand, SignedCommand};
use openbmt_core::identity::Identity;

use super::kit::*;
use super::net::{connect, start};
use crate::numbers::{Inbox, Outbox};
use crate::seal::{seal, Kind, Role};
use crate::server::ServerConfig;

fn command(seq: u64, signer: &Identity) -> SignedCommand {
    let command = OwnerCommand {
        counter: counter().device_id(),
        seq,
        action: Action::ClearCancelAlarm,
    };
    SignedCommand {
        owner_sig: command.sign(signer),
        command,
    }
}

fn roots() -> PinnedRoots {
    PinnedRoots::new(owner().device_id(), id(5).device_id()).unwrap()
}

#[test]
fn lk_09_an_owner_command_travels_through_a_relay_and_is_checked_as_a_command() {
    let server = start(ServerConfig::default());
    let mut counter_conn = connect(&server);
    counter_conn.make_mailbox(&counter()).unwrap();

    // The owner seals a signed command, with a number it stored first.
    let mut outbox = Outbox::default();
    let number = outbox
        .next(&counter().device_id(), |_| Ok::<(), ()>(()))
        .unwrap();
    let signed = command(3, &owner());
    let mut rng = Counting(0);
    let sealed = seal(
        &owner(),
        Role::Owner,
        &counter().device_id(),
        Role::Counter,
        number,
        Kind::Command,
        &signed.to_bytes(),
        &mut |b| rng.fill(b),
    )
    .unwrap();
    let mut owner_conn = connect(&server);
    owner_conn.put(&sealed).unwrap();

    // The counter fetches, takes the number, opens, and then verifies the
    // command by the command's own rules.
    let got = counter_conn.fetch(&counter(), 0).unwrap();
    let mut inbox = Inbox::default();
    let opened = inbox
        .open(&counter(), Role::Counter, &got.items[0].1, allowed, |_| {
            Ok::<(), ()>(())
        })
        .unwrap();
    assert_eq!(opened.kind, Kind::Command);
    let received = SignedCommand::from_bytes(&opened.content).unwrap();
    assert_eq!(received, signed);
    assert_eq!(received.verify(&roots(), &counter().device_id(), 0), Ok(()));
    // The command layer has its own number: the same command again is stale
    // even though the link would take it in a new message.
    assert!(matches!(
        received.verify(&roots(), &counter().device_id(), 3),
        Err(CommandError::Stale { .. })
    ));
    server.stop();
}

#[test]
fn lk_09_a_sealed_message_from_a_listed_device_is_still_checked_as_what_it_carries() {
    // A message that opens, from the owner's device key, carrying a command
    // signed by someone else: the link accepts it, the command does not.
    let sealed = {
        let mut rng = Counting(0);
        seal(
            &owner(),
            Role::Owner,
            &counter().device_id(),
            Role::Counter,
            1,
            Kind::Command,
            &command(1, &stranger()).to_bytes(),
            &mut |b| rng.fill(b),
        )
        .unwrap()
    };
    let opened = Inbox::default()
        .open(&counter(), Role::Counter, &sealed, allowed, |_| {
            Ok::<(), ()>(())
        })
        .unwrap();
    let received = SignedCommand::from_bytes(&opened.content).unwrap();
    assert_eq!(
        received.verify(&roots(), &counter().device_id(), 0),
        Err(CommandError::OwnerSignatureInvalid)
    );
    // And a command for another counter.
    let mut other = command(2, &owner());
    other.command.counter = the_box().device_id();
    other.owner_sig = other.command.sign(&owner());
    assert_eq!(
        other.verify(&roots(), &counter().device_id(), 0),
        Err(CommandError::WrongCounter)
    );
}
