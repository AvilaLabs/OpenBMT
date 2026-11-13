<?php
/**
 * Hex and base32 of bytes, strictly.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Bytes {

	/**
	 * Lower-case hex of bytes.
	 */
	public static function to_hex( string $bytes ): string {
		return bin2hex( $bytes );
	}

	/**
	 * Bytes of lower-case hex. Any other text, an odd length or upper case is
	 * refused.
	 *
	 * @throws Refusal When the text is not lower-case hex.
	 */
	public static function from_hex( string $text ): string {
		if ( 0 !== strlen( $text ) % 2 || 1 === preg_match( '/[^0-9a-f]/', $text ) ) {
			throw new Refusal( 'Hex' );
		}
		$bytes = hex2bin( $text );
		if ( false === $bytes ) {
			throw new Refusal( 'Hex' );
		}
		return $bytes;
	}

	/**
	 * Bytes of upper-case RFC 4648 base32 without padding. A character outside
	 * the alphabet and left-over bits that are not zero are refused.
	 *
	 * @throws Refusal When the text is not base32.
	 */
	public static function from_base32( string $text ): string {
		$alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
		$out      = '';
		$buffer   = 0;
		$bits     = 0;
		$length   = strlen( $text );
		for ( $i = 0; $i < $length; $i++ ) {
			$value = strpos( $alphabet, $text[ $i ] );
			if ( false === $value ) {
				throw new Refusal( 'Base32' );
			}
			$buffer = ( $buffer << 5 ) | $value;
			$bits  += 5;
			if ( $bits >= 8 ) {
				$bits  -= 8;
				$out   .= chr( ( $buffer >> $bits ) & 255 );
				$buffer &= ( 1 << $bits ) - 1;
			}
		}
		if ( $bits >= 5 || 0 !== $buffer ) {
			throw new Refusal( 'Base32' );
		}
		return $out;
	}
}
