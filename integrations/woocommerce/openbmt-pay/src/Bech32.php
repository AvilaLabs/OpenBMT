<?php
/**
 * The check of a segwit address: bech32 for version 0, bech32m for the
 * others, lower case only, the network prefixes bc, tb and bcrt.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Bech32 {

	const CHARSET = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';

	const MIN_BYTES = 14;

	const MAX_BYTES = 90;

	/**
	 * Whether the text is such an address.
	 */
	public static function is_address( string $text ): bool {
		$length = strlen( $text );
		if ( $length < self::MIN_BYTES || $length > self::MAX_BYTES || 1 === preg_match( '/[^0-9a-z]/', $text ) ) {
			return false;
		}
		$split = strrpos( $text, '1' );
		if ( false === $split ) {
			return false;
		}
		$hrp = substr( $text, 0, $split );
		if ( ! in_array( $hrp, array( 'bc', 'tb', 'bcrt' ), true ) ) {
			return false;
		}
		$data = array();
		$tail = substr( $text, $split + 1 );
		for ( $i = 0, $n = strlen( $tail ); $i < $n; $i++ ) {
			$value = strpos( self::CHARSET, $tail[ $i ] );
			if ( false === $value ) {
				return false;
			}
			$data[] = $value;
		}
		// Six characters of checksum and at least the version and a program.
		if ( count( $data ) < 8 ) {
			return false;
		}
		$version = $data[0];
		if ( $version > 16 ) {
			return false;
		}
		$constant = ( 0 === $version ) ? 1 : 0x2bc830a3;
		if ( self::polymod( array_merge( self::expand( $hrp ), $data ) ) !== $constant ) {
			return false;
		}
		$program = self::convert( array_slice( $data, 1, count( $data ) - 7 ) );
		if ( null === $program ) {
			return false;
		}
		$size = count( $program );
		if ( $size < 2 || $size > 40 ) {
			return false;
		}
		return 0 !== $version || 20 === $size || 32 === $size;
	}

	/**
	 * @param string $hrp The human-readable part.
	 * @return int[]
	 */
	private static function expand( string $hrp ): array {
		$out    = array();
		$length = strlen( $hrp );
		for ( $i = 0; $i < $length; $i++ ) {
			$out[] = ord( $hrp[ $i ] ) >> 5;
		}
		$out[] = 0;
		for ( $i = 0; $i < $length; $i++ ) {
			$out[] = ord( $hrp[ $i ] ) & 31;
		}
		return $out;
	}

	/**
	 * @param int[] $values Five-bit values.
	 */
	private static function polymod( array $values ): int {
		$generator = array( 0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3 );
		$check     = 1;
		foreach ( $values as $value ) {
			$top   = $check >> 25;
			$check = ( ( $check & 0x1ffffff ) << 5 ) ^ $value;
			for ( $i = 0; $i < 5; $i++ ) {
				if ( ( $top >> $i ) & 1 ) {
					$check ^= $generator[ $i ];
				}
			}
		}
		return $check;
	}

	/**
	 * Five-bit values into bytes. Padding of more than four bits, or not zero,
	 * gives null.
	 *
	 * @param int[] $values Five-bit values.
	 * @return int[]|null
	 */
	private static function convert( array $values ): ?array {
		$out    = array();
		$buffer = 0;
		$bits   = 0;
		foreach ( $values as $value ) {
			$buffer = ( $buffer << 5 ) | $value;
			$bits  += 5;
			if ( $bits >= 8 ) {
				$bits   -= 8;
				$out[]   = ( $buffer >> $bits ) & 255;
				$buffer &= ( 1 << $bits ) - 1;
			}
		}
		if ( $bits >= 5 || 0 !== $buffer ) {
			return null;
		}
		return $out;
	}
}
