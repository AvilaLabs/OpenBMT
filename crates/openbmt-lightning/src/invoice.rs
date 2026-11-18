//! What is asked of a Lightning invoice: the amount and lifetime of one sale
//! (LN-13), and the facts of a payment that is settled (LN-21).

use crate::error::AskRefused;

/// Millisatoshis in one satoshi.
pub const MSAT_PER_SAT: u64 = 1_000;
/// All the bitcoin there is, in millisatoshis.
pub const MAX_MSAT: u64 = 2_100_000_000_000_000 * MSAT_PER_SAT;
/// The longest lifetime of an invoice the crate asks for, in seconds.
pub const MAX_EXPIRY_SECS: u32 = 86_400;

/// The description of the invoice of a sale (LN-13): only the number of the
/// sale. The payer's wallet shows it, so it holds no shop name and no item.
pub fn sale_description(sale_number: u64) -> String {
    format!("Sale {sale_number}")
}

/// One invoice the counter asks for (LN-13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    amount_msat: u64,
    expiry_secs: u32,
    description: String,
}

impl Ask {
    /// The invoice of a sale: `amount_msat` (the sale's satoshis times
    /// 1,000), as long as the counter's quote lasts.
    ///
    /// `expiry_secs` is the quote's lifetime (`openbmt_counter::money::
    /// QUOTE_SECS`, 300, CR-05). It is a parameter because it lives in the
    /// counter crate, which this one does not depend on.
    pub fn sale(sale_number: u64, amount_msat: u64, expiry_secs: u32) -> Result<Self, AskRefused> {
        Self::checked(amount_msat, expiry_secs, sale_description(sale_number))
    }

    fn checked(
        amount_msat: u64,
        expiry_secs: u32,
        description: String,
    ) -> Result<Self, AskRefused> {
        if amount_msat == 0 || amount_msat > MAX_MSAT {
            return Err(AskRefused::BadAmount);
        }
        if expiry_secs == 0 || expiry_secs > MAX_EXPIRY_SECS {
            return Err(AskRefused::BadExpiry);
        }
        Ok(Self {
            amount_msat,
            expiry_secs,
            description,
        })
    }

    /// The amount, in millisatoshis.
    pub fn amount_msat(&self) -> u64 {
        self.amount_msat
    }

    /// The lifetime of the invoice, in seconds.
    pub fn expiry_secs(&self) -> u32 {
        self.expiry_secs
    }

    /// The description the invoice must carry.
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// The facts of a payment that is settled (LN-21). Lightning payments
/// are final at once (DESIGN section 8), so these are all the counter keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaidFacts {
    /// The payment hash of the invoice.
    pub payment_hash: [u8; 32],
    /// What the node received, in millisatoshis: at least what was asked.
    pub amount_received_msat: u64,
    /// When the node says it settled, in seconds since 1970, if it says.
    pub settled_at: Option<u64>,
}
