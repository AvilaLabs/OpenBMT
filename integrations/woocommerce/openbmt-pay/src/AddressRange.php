<?php
/**
 * WO-10: a signed range of addresses. Bytes: the magic, the version, the id
 * of the pairing (8 bytes), the index of the first address (u32), the number
 * of addresses (1 byte), each address as a length byte and its ASCII bytes,
 * and the 64-byte signature of everything before it.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class AddressRange {

	const MAGIC = 'OBM{';

	const VERSION = 1;

	const MAX_SIZE = 100;

	/**
	 * Opens a signed range under a public key.
	 *
	 * @param string $frame  The bytes sent.
	 * @param string $public The owner phone's public key.
	 * @param string $id     The id of the pairing the store holds (8 bytes).
	 * @return array{start:int,addresses:string[]}
	 * @throws Refusal 'Range' when the bytes are not a range, 'Signature' when the signature does not match, 'Pairing' when it is for another pairing.
	 */
	public static function open( string $frame, string $public, string $id ): array {
		if ( strlen( $frame ) <= SODIUM_CRYPTO_SIGN_BYTES || SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES !== strlen( $public ) ) {
			throw new Refusal( 'Range' );
		}
		$body      = substr( $frame, 0, -SODIUM_CRYPTO_SIGN_BYTES );
		$signature = substr( $frame, -SODIUM_CRYPTO_SIGN_BYTES );
		$range     = self::read_body( $body );
		if ( ! sodium_crypto_sign_verify_detached( $signature, $body, $public ) ) {
			throw new Refusal( 'Signature' );
		}
		if ( 8 !== strlen( $id ) || ! hash_equals( $id, $range['pairing_id'] ) ) {
			throw new Refusal( 'Pairing' );
		}
		return array(
			'start'     => $range['start'],
			'addresses' => $range['addresses'],
		);
	}

	/**
	 * @return array{pairing_id:string,start:int,addresses:string[]}
	 * @throws Refusal When the bytes are not a range.
	 */
	private static function read_body( string $body ): array {
		try {
			$reader = new Reader( $body );
			if ( self::MAGIC !== $reader->take( 4 ) || self::VERSION !== $reader->u8() ) {
				throw new Refusal( 'Range' );
			}
			$pairing   = $reader->take( 8 );
			$start     = $reader->u32();
			$count     = $reader->u8();
			$addresses = array();
			for ( $i = 0; $i < $count; $i++ ) {
				$addresses[] = $reader->take( $reader->u8() );
			}
			if ( ! $reader->is_empty() ) {
				throw new Refusal( 'Range' );
			}
		} catch ( Refusal $e ) {
			throw new Refusal( 'Range' );
		}
		if ( 0 === $count || $count > self::MAX_SIZE || $start + $count > 0xffffffff ) {
			throw new Refusal( 'Range' );
		}
		foreach ( $addresses as $address ) {
			if ( ! Bech32::is_address( $address ) ) {
				throw new Refusal( 'Range' );
			}
		}
		return array(
			'pairing_id' => $pairing,
			'start'      => $start,
			'addresses'  => $addresses,
		);
	}
}
