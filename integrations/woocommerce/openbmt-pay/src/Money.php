<?php
/**
 * Amounts as text: an order total into minor units, and satoshis into the
 * bitcoin text of a payment link.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Money {

	/**
	 * The minor units of an order total written as decimal text, such as
	 * "12.34". More decimals than the currency has are refused, and so is a
	 * number too large to count exactly.
	 *
	 * @throws Refusal 'Amount' for any other text.
	 */
	public static function minor_from_decimal( string $text, string $currency ): int {
		$digits = Rates::minor_digits( $currency );
		$shape  = ( 0 === $digits ) ? '/^[0-9]{1,15}$/' : '/^[0-9]{1,15}(\.[0-9]{1,' . $digits . '})?$/';
		if ( 1 !== preg_match( $shape, $text ) ) {
			throw new Refusal( 'Amount' );
		}
		$parts = explode( '.', $text, 2 );
		$whole = (int) $parts[0];
		$frac  = str_pad( $parts[1] ?? '', $digits, '0' );
		return $whole * ( 10 ** $digits ) + ( '' === $frac ? 0 : (int) $frac );
	}

	/**
	 * Satoshis as bitcoin: at most 8 decimals, no trailing zeros, no exponent.
	 */
	public static function btc_text( int $sats ): string {
		$whole = intdiv( $sats, Quote::SATS_PER_BTC );
		$frac  = rtrim( str_pad( (string) ( $sats % Quote::SATS_PER_BTC ), 8, '0', STR_PAD_LEFT ), '0' );
		return '' === $frac ? (string) $whole : $whole . '.' . $frac;
	}
}
