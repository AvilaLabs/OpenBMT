//! SS-30, SS-31, SS-42: the starter wallet pays only a refund that was
//! queued, and the one move of everything to the new wallet. The seed is
//! opened for one call, the finished transaction is checked to pay only
//! what it should (as FL-21 does for the refund float), and nothing secret
//! comes back.

use bitcoin::bip32::ChildNumber;
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{ecdsa, OutPoint, PublicKey, ScriptBuf, Transaction, Txid};
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_spend::{
    check_finished, finalize_plan, plan, ChangePosition, Coin, Plan, PlanError, SpendRequest,
};

use super::chance::Opened;
use super::seal::{open_seed, DataKey};
use super::StarterRefusal;

/// SS-30: the least fee rate of a payment, in satoshis per 1000 virtual
/// bytes (1 sat/vB), as `docs/OWNER-COINS-RULES.md` OC-17 sets the least.
pub const MIN_RATE_PER_KVB: u64 = 1_000;

/// SS-30, SS-42: the fee rate of a payment, in satoshis per 1000 virtual
/// bytes: the recent rate of the blocks the device checked
/// (`FeeHistory::recent_rate`, `docs/FEE-RULES.md` FE-13) and at least 1
/// sat/vB; when there is none, the rate the owner typed, and never 0. With
/// neither, no rate. The same rule as the refund float's.
pub fn fee_rate(recent: Option<u64>, typed: Option<u64>) -> Result<u64, StarterRefusal> {
    match (recent, typed) {
        (Some(r), _) => Ok(r.max(MIN_RATE_PER_KVB)),
        (None, Some(t)) if t > 0 => Ok(t),
        _ => Err(StarterRefusal::NoFeeRate),
    }
}

/// What [`pay_refund`] and [`move_all`] hand back: the finished
/// transaction, ready for the outbox, and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterPaid {
    /// The finished, signed transaction.
    pub tx: Transaction,
    /// Its ID.
    pub txid: Txid,
    /// The fee in satoshis.
    pub fee: u64,
    /// The virtual size.
    pub vsize: u64,
    /// The amount paid to the destination.
    pub paid: u64,
    /// The starter wallet's change: its index on the change chain and its
    /// amount.
    pub change: Option<(u32, u64)>,
    /// The coins spent.
    pub spent: Vec<OutPoint>,
}

/// SS-42: the script of the new wallet at `index` on its receive chain, from
/// its description (the next unused index is the caller's, as the library
/// has no chain). A 2-of-3 wallet and a single-key one both work.
pub fn receive_script_of(wallet: &ImportedWallet, index: u32) -> Result<ScriptBuf, StarterRefusal> {
    wallet
        .receive_script(index)
        .map_err(|_| StarterRefusal::NewAddress)
}

/// SS-30: pay `sats` to `pay_to` (a queued refund, OA-115) from the starter
/// wallet's own `coins`, with change to its change script at
/// `change_index`. `fee_rate` is in satoshis per 1000 virtual bytes
/// ([`fee_rate`]); `min_confirmations` is the fewest a coin needs. A refund
/// that the coins cannot cover with its fee is refused and never split.
#[allow(clippy::too_many_arguments)]
pub fn pay_refund(
    sealed: &[u8],
    key: &DataKey,
    coins: &[Coin],
    pay_to: &ScriptBuf,
    sats: u64,
    fee_rate: u64,
    change_index: u32,
    min_confirmations: u32,
    lock_height: u32,
) -> Result<StarterPaid, StarterRefusal> {
    let opened = open_seed(sealed, key)?.open()?;
    let request = SpendRequest {
        payments: vec![(pay_to.clone(), sats)],
        fee_rate,
        change_index,
        min_confirmations,
        send_all: false,
        must_spend: Vec::new(),
        lock_height,
        change_position: ChangePosition::Last,
    };
    spend(&opened, coins, &request, Some(sats))
}

/// SS-42: move every coin of the starter wallet to the new wallet's
/// receive address `new_index`, in one payment with no change. Every coin
/// must be confirmed to `min_confirmations`: a coin that is not is refused,
/// as it would be left behind. The new wallet must not be the starter
/// wallet itself.
#[allow(clippy::too_many_arguments)]
pub fn move_all(
    sealed: &[u8],
    key: &DataKey,
    coins: &[Coin],
    new_wallet: &ImportedWallet,
    new_index: u32,
    fee_rate: u64,
    min_confirmations: u32,
    lock_height: u32,
) -> Result<StarterPaid, StarterRefusal> {
    if coins.is_empty() {
        return Err(StarterRefusal::NothingToMove);
    }
    if coins.iter().any(|c| c.confirmations < min_confirmations) {
        return Err(StarterRefusal::Unconfirmed);
    }
    let opened = open_seed(sealed, key)?.open()?;
    if let ImportedWallet::Single(w) = new_wallet {
        if w == opened.wallet.single() {
            return Err(StarterRefusal::NotOnlyThese(
                "the new wallet is the starter wallet".to_string(),
            ));
        }
    }
    let to = receive_script_of(new_wallet, new_index)?;
    let request = SpendRequest {
        payments: vec![(to, 0)],
        fee_rate,
        change_index: 0,
        min_confirmations,
        send_all: true,
        must_spend: Vec::new(),
        lock_height,
        change_position: ChangePosition::Last,
    };
    spend(&opened, coins, &request, None)
}

fn plan_refusal(e: PlanError, sats: u64) -> StarterRefusal {
    match e {
        PlanError::InsufficientFunds { available, needed } => {
            StarterRefusal::NotEnough { available, needed }
        }
        PlanError::NoEligibleCoins => StarterRefusal::NotEnough {
            available: 0,
            needed: sats,
        },
        PlanError::Coin(c) => StarterRefusal::NotStarterCoin(c.to_string()),
        other => StarterRefusal::Plan(other.to_string()),
    }
}

fn spend(
    opened: &Opened,
    coins: &[Coin],
    request: &SpendRequest,
    sats: Option<u64>,
) -> Result<StarterPaid, StarterRefusal> {
    let wallet = opened.wallet.imported();
    let planned = plan(&wallet, coins, request).map_err(|e| plan_refusal(e, sats.unwrap_or(0)))?;
    let mut psbt = planned
        .to_psbt(&wallet)
        .map_err(|e| StarterRefusal::Sign(e.to_string()))?;
    sign_inputs(opened, &planned, &mut psbt)?;
    let tx =
        finalize_plan(&wallet, &psbt, &planned).map_err(|e| StarterRefusal::Sign(e.to_string()))?;
    let fee =
        check_finished(&wallet, &psbt, &tx).map_err(|e| StarterRefusal::Sign(e.to_string()))?;
    let pay = request
        .payments
        .first()
        .ok_or_else(|| StarterRefusal::Plan("no payment".to_string()))?;
    let change_script = opened
        .wallet
        .script(Keychain::Change, request.change_index)?;
    // The coins the check holds the transaction to: those the plan spent,
    // and for a move, every coin it was given.
    let used: Vec<&Coin> = if request.send_all {
        coins.iter().collect()
    } else {
        planned.inputs().iter().map(|i| &i.coin).collect()
    };
    check_only_these(
        opened.wallet.single(),
        &used,
        &tx,
        &pay.0,
        sats,
        &change_script,
        request.send_all,
    )?;
    let paid: u64 = tx
        .output
        .iter()
        .filter(|o| o.script_pubkey == pay.0)
        .map(|o| o.value.to_sat())
        .sum();
    let change = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == change_script && !request.send_all)
        .map(|o| (request.change_index, o.value.to_sat()));
    Ok(StarterPaid {
        txid: tx.compute_txid(),
        vsize: u64::try_from(tx.vsize()).unwrap_or(u64::MAX),
        fee,
        paid,
        change,
        spent: tx.input.iter().map(|i| i.previous_output).collect(),
        tx,
    })
}

/// SS-30: sign every input with the starter wallet's key, `SIGHASH_ALL`,
/// P2WPKH.
fn sign_inputs(opened: &Opened, planned: &Plan, psbt: &mut Psbt) -> Result<(), StarterRefusal> {
    let secp = Secp256k1::new();
    let unsigned = psbt.unsigned_tx.clone();
    let mut cache = SighashCache::new(&unsigned);
    for (n, (entry, input)) in psbt.inputs.iter_mut().zip(planned.inputs()).enumerate() {
        let coin = &input.coin;
        let output = coin
            .output()
            .ok_or_else(|| StarterRefusal::Sign("a coin has no output".to_string()))?;
        let branch = match coin.chain {
            Keychain::Receive => 0,
            Keychain::Change => 1,
        };
        let mut child = opened
            .account
            .derive_priv(
                &secp,
                &[
                    ChildNumber::Normal { index: branch },
                    ChildNumber::Normal { index: coin.index },
                ],
            )
            .map_err(|_| StarterRefusal::Sign("the key cannot be derived".to_string()))?;
        let hash = cache
            .p2wpkh_signature_hash(
                n,
                &output.script_pubkey,
                output.value,
                EcdsaSighashType::All,
            )
            .map_err(|e| StarterRefusal::Sign(e.to_string()));
        let hash = match hash {
            Ok(h) => h,
            Err(e) => {
                child.private_key.non_secure_erase();
                return Err(e);
            }
        };
        let message = Message::from_digest(hash.to_byte_array());
        let signature = secp.sign_ecdsa(&message, &child.private_key);
        let key = PublicKey::new(child.private_key.public_key(&secp));
        child.private_key.non_secure_erase();
        entry.partial_sigs.insert(
            key,
            ecdsa::Signature {
                signature,
                sighash_type: EcdsaSighashType::All,
            },
        );
    }
    Ok(())
}

/// SS-30, SS-42: the finished transaction `tx` spends only coins of the
/// starter wallet (each of `coins`, whose scripts are the wallet's) and
/// pays only `pay_to` (with `pay_sats` when it is a payment of a set
/// amount) and the wallet's own change script `change`. A move (`sweep`
/// set) pays `pay_to` alone and spends every coin it was given.
pub fn check_only_these(
    wallet: &SingleKeyWallet,
    coins: &[&Coin],
    tx: &Transaction,
    pay_to: &ScriptBuf,
    pay_sats: Option<u64>,
    change: &ScriptBuf,
    sweep: bool,
) -> Result<(), StarterRefusal> {
    let bad = |why: &str| StarterRefusal::NotOnlyThese(why.to_string());
    if tx.input.is_empty() {
        return Err(bad("the transaction spends nothing"));
    }
    let own = ImportedWallet::Single(wallet.clone());
    for input in &tx.input {
        let coin = coins
            .iter()
            .find(|c| c.outpoint == input.previous_output)
            .ok_or_else(|| bad("an input is not a coin of the starter wallet"))?;
        coin.check(&own)
            .map_err(|e| StarterRefusal::NotStarterCoin(e.to_string()))?;
    }
    if sweep && coins.len() != tx.input.len() {
        return Err(bad("a move must spend every coin"));
    }
    if pay_to == change {
        return Err(bad("the payment is to the starter wallet's own change"));
    }
    let mut paid = 0usize;
    let mut changes = 0usize;
    for output in &tx.output {
        if output.script_pubkey == *pay_to {
            paid += 1;
            if let Some(sats) = pay_sats {
                if output.value.to_sat() != sats {
                    return Err(bad("the payment is not the amount asked"));
                }
            }
        } else if output.script_pubkey == *change && !sweep {
            changes += 1;
        } else {
            return Err(bad("an output pays something else"));
        }
    }
    if paid != 1 {
        return Err(bad("the destination must be paid exactly once"));
    }
    if changes > 1 {
        return Err(bad("more than one change output"));
    }
    Ok(())
}
