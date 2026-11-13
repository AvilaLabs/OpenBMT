<?php
/**
 * The text of a price into minor units per bitcoin (RT-20 to RT-24), worked
 * out exactly in integers and rounded half up.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Price {

	const MAX_DECIMALS = 8;

	/**
	 * A price as the text of a source gives it.
	 *
	 * @param string $text     The price.
	 * @param string $currency The currency code.
	 * @return int Minor units per bitcoin.
	 * @throws Refusal With the reason of RT-20 to RT-24, named as in the Rust library.
	 */
	public static function parse_minor( string $text, string $currency ): int {
		$band = Rates::band( $currency );
		if ( null === $band ) {
			throw new Refusal( 'OutOfBand' );
		}
		list( $min, $max ) = $band;
		if ( '' === $text ) {
			throw new Refusal( 'Empty' );
		}
		if ( '+' === $text[0] || '-' === $text[0] ) {
			throw new Refusal( 'Sign' );
		}
		if ( 1 === preg_match( '/[eE]/', $text ) ) {
			throw new Refusal( 'Exponent' );
		}
		if ( 1 === preg_match( '/[^0-9.]/', $text ) ) {
			throw new Refusal( 'BadCharacter' );
		}
		if ( substr_count( $text, '.' ) > 1 ) {
			throw new Refusal( 'TwoDots' );
		}
		$parts    = explode( '.', $text, 2 );
		$whole    = $parts[0];
		$has_dot  = isset( $parts[1] );
		$fraction = $has_dot ? $parts[1] : '';
		if ( '' === $whole || ( $has_dot && '' === $fraction ) ) {
			throw new Refusal( 'BadShape' );
		}
		if ( strlen( $whole ) > 1 && '0' === $whole[0] ) {
			throw new Refusal( 'LeadingZero' );
		}
		if ( strlen( $fraction ) > self::MAX_DECIMALS ) {
			throw new Refusal( 'TooManyDecimals' );
		}
		$units = 0;
		for ( $i = 0, $n = strlen( $whole ); $i < $n; $i++ ) {
			$units = $units * 10 + (int) $whole[ $i ];
			if ( $units > $max ) {
				throw new Refusal( 'OutOfBand' );
			}
		}
		$digits = Rates::minor_digits( $currency );
		$frac8  = (int) str_pad( $fraction, self::MAX_DECIMALS, '0' );
		$unit   = 10 ** ( self::MAX_DECIMALS - $digits );
		$minor  = $units * ( 10 ** $digits ) + intdiv( $frac8 + intdiv( $unit, 2 ), $unit );
		if ( 0 === $minor ) {
			throw new Refusal( 'Zero' );
		}
		if ( $minor < $min || $minor > $max ) {
			throw new Refusal( 'OutOfBand' );
		}
		return $minor;
	}
}
