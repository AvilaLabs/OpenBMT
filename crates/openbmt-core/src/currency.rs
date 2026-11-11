//! The currency a shop prices in (`docs/COUNTER-RULES.md`, CR-193 to
//! CR-199): one of a fixed list, or Bitcoin Standard, which prices in
//! satoshis and needs no price. The settings hold it; the rates, the
//! counter and the apps read it from here.

/// What a shop prices its sales in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShopCurrency {
    /// US dollar.
    Usd,
    /// Euro.
    Eur,
    /// Pound sterling.
    Gbp,
    /// Japanese yen.
    Jpy,
    /// Australian dollar.
    Aud,
    /// Indian rupee.
    Inr,
    /// South Korean won.
    Krw,
    /// Indonesian rupiah.
    Idr,
    /// Singapore dollar.
    Sgd,
    /// Bitcoin Standard: prices in satoshis, no price source.
    Sats,
}

impl ShopCurrency {
    /// Every choice, in the order the owner is shown them.
    pub const ALL: [ShopCurrency; 10] = [
        Self::Usd,
        Self::Eur,
        Self::Gbp,
        Self::Jpy,
        Self::Aud,
        Self::Inr,
        Self::Krw,
        Self::Idr,
        Self::Sgd,
        Self::Sats,
    ];

    /// The three capital letters a sale records: the ISO 4217 code, or
    /// `SAT` for Bitcoin Standard.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Usd => "USD",
            Self::Eur => "EUR",
            Self::Gbp => "GBP",
            Self::Jpy => "JPY",
            Self::Aud => "AUD",
            Self::Inr => "INR",
            Self::Krw => "KRW",
            Self::Idr => "IDR",
            Self::Sgd => "SGD",
            Self::Sats => "SAT",
        }
    }

    /// The choice with these three letters.
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.code() == code)
    }

    /// The byte the settings encoding writes: 1 for the first choice and so
    /// on. 0 is not a choice.
    pub fn to_byte(&self) -> u8 {
        match self {
            Self::Usd => 1,
            Self::Eur => 2,
            Self::Gbp => 3,
            Self::Jpy => 4,
            Self::Aud => 5,
            Self::Inr => 6,
            Self::Krw => 7,
            Self::Idr => 8,
            Self::Sgd => 9,
            Self::Sats => 10,
        }
    }

    /// The choice a settings byte names.
    pub fn from_byte(byte: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.to_byte() == byte)
    }

    /// ISO 4217 minor-unit digits: how many places an amount in minor units
    /// has after the point. Yen and won have none, and so do satoshis.
    pub fn minor_digits(&self) -> u32 {
        match self {
            Self::Jpy | Self::Krw | Self::Sats => 0,
            _ => 2,
        }
    }

    /// Whether this is Bitcoin Standard: amounts are satoshis and no price
    /// is used.
    pub fn is_sats(&self) -> bool {
        matches!(self, Self::Sats)
    }

    /// The currency's name in the plural, as a sentence says it where it
    /// once said "dollars".
    pub fn plural(&self) -> &'static str {
        match self {
            Self::Usd => "dollars",
            Self::Eur => "euros",
            Self::Gbp => "pounds",
            Self::Jpy => "yen",
            Self::Aud => "Australian dollars",
            Self::Inr => "rupees",
            Self::Krw => "won",
            Self::Idr => "rupiah",
            Self::Sgd => "Singapore dollars",
            Self::Sats => "sats",
        }
    }

    /// The choice as the owner reads it: `US dollar (USD)`.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Usd => "US dollar (USD)",
            Self::Eur => "Euro (EUR)",
            Self::Gbp => "Pound sterling (GBP)",
            Self::Jpy => "Japanese yen (JPY)",
            Self::Aud => "Australian dollar (AUD)",
            Self::Inr => "Indian rupee (INR)",
            Self::Krw => "South Korean won (KRW)",
            Self::Idr => "Indonesian rupiah (IDR)",
            Self::Sgd => "Singapore dollar (SGD)",
            Self::Sats => "Bitcoin Standard (sats)",
        }
    }

    /// The factor that the band of a price per bitcoin is the dollar band
    /// times, in minor units (`docs/RATE-RULES.md`, RT-24). `None` for
    /// Bitcoin Standard, which has no price.
    pub fn band_factor(&self) -> Option<u64> {
        match self {
            Self::Usd | Self::Eur | Self::Gbp | Self::Aud | Self::Sgd => Some(1),
            Self::Jpy | Self::Inr => Some(100),
            Self::Krw => Some(1_000),
            Self::Idr => Some(10_000),
            Self::Sats => None,
        }
    }

    /// The band of a price per bitcoin in whole units of the currency: the
    /// band of `docs/RATE-RULES.md` (RT-24), in minor units, divided by the
    /// minor unit. `None` for Bitcoin Standard.
    pub fn band_whole(&self) -> Option<(u64, u64)> {
        let factor = self.band_factor()?;
        let unit = 10u64.pow(self.minor_digits());
        Some((
            10_000u64.checked_mul(factor)? / unit,
            10_000_000_000u64.checked_mul(factor)? / unit,
        ))
    }

    /// A whole number of this currency written for a person: `$1,250` for
    /// dollars (as before), `1,250 euros` for the others, `1,250 sats` for
    /// Bitcoin Standard.
    pub fn whole_text(&self, n: u64) -> String {
        let n = grouped(n);
        match self {
            Self::Usd => format!("${n}"),
            other => format!("{n} {}", other.plural()),
        }
    }
}

/// A number with a comma between thousands.
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cu_01_the_list_has_ten_choices_with_distinct_codes_and_bytes() {
        assert_eq!(ShopCurrency::ALL.len(), 10);
        for (i, a) in ShopCurrency::ALL.iter().enumerate() {
            assert_eq!(a.code().len(), 3);
            assert!(a.code().bytes().all(|b| b.is_ascii_uppercase()));
            assert_eq!(ShopCurrency::from_code(a.code()), Some(*a));
            assert_eq!(ShopCurrency::from_byte(a.to_byte()), Some(*a));
            assert_eq!(usize::from(a.to_byte()), i + 1);
            for b in ShopCurrency::ALL.iter().skip(i + 1) {
                assert_ne!(a.code(), b.code());
                assert_ne!(a.label(), b.label());
            }
        }
        assert_eq!(ShopCurrency::from_byte(0), None);
        assert_eq!(ShopCurrency::from_byte(11), None);
        assert_eq!(ShopCurrency::from_code("CHF"), None);
        assert_eq!(ShopCurrency::from_code("usd"), None);
    }

    #[test]
    fn cu_02_minor_digits_follow_iso_4217() {
        for c in ShopCurrency::ALL {
            let want = match c {
                ShopCurrency::Jpy | ShopCurrency::Krw | ShopCurrency::Sats => 0,
                _ => 2,
            };
            assert_eq!(c.minor_digits(), want, "{}", c.code());
        }
    }

    #[test]
    fn cu_03_whole_amounts_are_written_with_the_currency_name() {
        assert_eq!(ShopCurrency::Usd.whole_text(1_250), "$1,250");
        assert_eq!(ShopCurrency::Eur.whole_text(1_250), "1,250 euros");
        assert_eq!(ShopCurrency::Sats.whole_text(21), "21 sats");
        assert_eq!(ShopCurrency::Jpy.whole_text(0), "0 yen");
        assert_eq!(ShopCurrency::Sats.plural(), "sats");
        assert!(ShopCurrency::Sats.is_sats());
        assert!(!ShopCurrency::Usd.is_sats());
    }

    #[test]
    fn rt_24_the_band_in_whole_units_follows_the_minor_unit() {
        assert_eq!(ShopCurrency::Usd.band_whole(), Some((100, 100_000_000)));
        assert_eq!(
            ShopCurrency::Jpy.band_whole(),
            Some((1_000_000, 1_000_000_000_000))
        );
        assert_eq!(
            ShopCurrency::Inr.band_whole(),
            Some((10_000, 10_000_000_000))
        );
        assert_eq!(
            ShopCurrency::Krw.band_whole(),
            Some((10_000_000, 10_000_000_000_000))
        );
        assert_eq!(
            ShopCurrency::Idr.band_whole(),
            Some((1_000_000, 1_000_000_000_000))
        );
        assert_eq!(ShopCurrency::Sats.band_whole(), None);
    }

    #[test]
    fn rt_24_the_band_factors_are_the_fixed_table() {
        let want = [
            (ShopCurrency::Usd, Some(1)),
            (ShopCurrency::Eur, Some(1)),
            (ShopCurrency::Gbp, Some(1)),
            (ShopCurrency::Aud, Some(1)),
            (ShopCurrency::Sgd, Some(1)),
            (ShopCurrency::Jpy, Some(100)),
            (ShopCurrency::Inr, Some(100)),
            (ShopCurrency::Krw, Some(1_000)),
            (ShopCurrency::Idr, Some(10_000)),
            (ShopCurrency::Sats, None),
        ];
        for (c, f) in want {
            assert_eq!(c.band_factor(), f, "{}", c.code());
        }
    }
}
