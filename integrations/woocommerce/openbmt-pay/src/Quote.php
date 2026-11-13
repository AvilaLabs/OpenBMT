<?php
/**
 * WO-15: the quote of a web order, made by the rules of the counter's quote
 * in docs/RATE-RULES.md: several prices into one rate (RT-30 to RT-38) and
 * the satoshis an amount needs at a rate, rounded up (CR-08).
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Quote {

	/** The seconds a quote lasts (WO-15). */
	const LIFETIME_SECS = 1800;

	/** The oldest a price may be, in seconds before now (CR-58). */
	const MAX_AGE_SECS = 600;

	/** The furthest ahead of now a price may be, in seconds (CR-58). */
	const MAX_AHEAD_SECS = 60;

	/** The most a price may differ from the median, in hundredths of a percent (RT-33). */
	const MAX_SPREAD_BPS = 200;

	/** The fewest distinct sources (RT-31). */
	const MIN_SOURCES = 2;

	const SATS_PER_BTC = 100000000;

	/** The most satoshis there are. */
	const MAX_SATS = 2100000000000000;

	/**
	 * A sample: a source, when the reply came and the price in minor units.
	 *
	 * @return array{id:string,fetched_at:int,price:int}
	 * @throws Refusal 'UnknownSource', or 'OutOfBand' for a price outside the band of its currency.
	 */
	public static function sample( string $id, int $fetched_at, int $price ): array {
		$source = Rates::source( $id );
		if ( null === $source ) {
			throw new Refusal( 'UnknownSource' );
		}
		$band = Rates::band( $source[2] );
		if ( null === $band || $price < $band[0] || $price > $band[1] ) {
			throw new Refusal( 'OutOfBand' );
		}
		return array(
			'id'         => $id,
			'fetched_at' => $fetched_at,
			'price'      => $price,
		);
	}

	/**
	 * RT-30 to RT-38: the rate from the samples at the time $now.
	 *
	 * @param array<int,array{id:string,fetched_at:int,price:int}> $samples The samples.
	 * @param int                                                  $now     Unix seconds.
	 * @return array{rate:int,label:string,oldest:int}
	 * @throws Refusal 'TooOld', 'NotEnoughSources', 'PricesDisagree' or 'MixedCurrencies'.
	 */
	public static function combine( array $samples, int $now ): array {
		$used = array();
		foreach ( $samples as $s ) {
			if ( $s['fetched_at'] < $now - self::MAX_AGE_SECS || $s['fetched_at'] > $now + self::MAX_AHEAD_SECS ) {
				continue;
			}
			if ( ! isset( $used[ $s['id'] ] ) || $used[ $s['id'] ]['fetched_at'] < $s['fetched_at'] ) {
				$used[ $s['id'] ] = $s;
			}
		}
		if ( array() === $used && array() !== $samples ) {
			throw new Refusal( 'TooOld' );
		}
		if ( count( $used ) < self::MIN_SOURCES ) {
			throw new Refusal( 'NotEnoughSources', array( 'fresh' => count( $used ) ) );
		}
		$currencies = array();
		foreach ( $used as $s ) {
			$source = Rates::source( $s['id'] );
			if ( null === $source ) {
				throw new Refusal( 'UnknownSource' );
			}
			$currencies[ $source[2] ] = true;
		}
		if ( count( $currencies ) > 1 ) {
			throw new Refusal( 'MixedCurrencies' );
		}
		$prices = array();
		foreach ( $used as $s ) {
			$prices[] = $s['price'];
		}
		sort( $prices );
		$median  = $prices[ intdiv( count( $prices ) - 1, 2 ) ];
		$lowest  = $prices[0];
		$highest = $prices[ count( $prices ) - 1 ];
		foreach ( $prices as $p ) {
			if ( abs( $p - $median ) * 10000 > $median * self::MAX_SPREAD_BPS ) {
				throw new Refusal(
					'PricesDisagree',
					array(
						'lowest'  => $lowest,
						'highest' => $highest,
					)
				);
			}
		}
		$ids = array_keys( $used );
		usort(
			$ids,
			static function ( string $a, string $b ): int {
				return Rates::position( $a ) <=> Rates::position( $b );
			}
		);
		$label  = 'med.';
		$oldest = PHP_INT_MAX;
		foreach ( $ids as $id ) {
			$source = Rates::source( $id );
			$label .= $source ? $source[1] : '';
			$oldest = min( $oldest, $used[ $id ]['fetched_at'] );
		}
		return array(
			'rate'   => $median,
			'label'  => $label,
			'oldest' => $oldest,
		);
	}

	/**
	 * CR-08: the satoshis an amount needs at a rate, rounded up to a whole
	 * satoshi: ceil(amount * 100,000,000 / rate), in integers that cannot
	 * overflow. Null when the answer is above 21,000,000 bitcoin.
	 *
	 * @param int $amount Minor units of the currency.
	 * @param int $rate   Minor units per bitcoin.
	 */
	public static function sats_for( int $amount, int $rate ): ?int {
		if ( $amount < 0 || $rate <= 0 ) {
			return null;
		}
		$whole = intdiv( $amount, $rate );
		$rest  = $amount % $rate;
		if ( $whole > intdiv( self::MAX_SATS, self::SATS_PER_BTC ) ) {
			return null;
		}
		$fraction = 0;
		for ( $i = 0; $i < 8; $i++ ) {
			$rest    *= 10;
			$fraction = $fraction * 10 + intdiv( $rest, $rate );
			$rest     = $rest % $rate;
		}
		$sats = $whole * self::SATS_PER_BTC + $fraction + ( $rest > 0 ? 1 : 0 );
		return $sats <= self::MAX_SATS ? $sats : null;
	}

	/**
	 * The quote of an order: the readings of the shop's sources, as the
	 * price texts they gave, into the satoshis the order total needs. A
	 * source whose text is refused gives no sample.
	 *
	 * @param string                                       $currency     The store's currency code.
	 * @param int                                          $amount_minor The order total in minor units.
	 * @param array<int,array{0:string,1:int,2:string}>    $readings     Source id, when it was read, price text.
	 * @param int                                          $now          Unix seconds.
	 * @return array{sats:int,rate:int,label:string,quoted_at:int,expires_at:int}
	 * @throws Refusal When no quote can be made.
	 */
	public static function make( string $currency, int $amount_minor, array $readings, int $now ): array {
		if ( $amount_minor <= 0 ) {
			throw new Refusal( 'Zero' );
		}
		$samples = array();
		foreach ( $readings as $reading ) {
			$source = Rates::source( $reading[0] );
			if ( null === $source || $source[2] !== $currency ) {
				throw new Refusal( 'MixedCurrencies' );
			}
			try {
				$price = Price::parse_minor( $reading[2], $currency );
			} catch ( Refusal $e ) {
				continue;
			}
			$samples[] = self::sample( $reading[0], $reading[1], $price );
		}
		$rate = self::combine( $samples, $now );
		$sats = self::sats_for( $amount_minor, $rate['rate'] );
		if ( null === $sats || $sats <= 0 ) {
			throw new Refusal( 'Amount' );
		}
		return array(
			'sats'       => $sats,
			'rate'       => $rate['rate'],
			'label'      => $rate['label'],
			'quoted_at'  => $now,
			'expires_at' => $now + self::LIFETIME_SECS,
		);
	}
}
