<?php
/**
 * WO-30: a signed notice. Bytes: the magic, the version, the order number
 * (u64), the address (length byte and ASCII), the state (1 byte), the amount
 * counted in satoshis (u64), the depth (u32), the number of transaction IDs
 * (1 byte), each ID as 32 bytes, and the 64-byte signature of everything
 * before it. All numbers are big endian.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Notice {

	const MAGIC = 'OBM}';

	const VERSION = 1;

	const MAX_TXIDS = 16;

	const STATES = array(
		1 => 'paid',
		2 => 'overpaid',
		3 => 'part_paid',
		4 => 'paid_late',
	);

	/**
	 * Opens a signed notice under a public key.
	 *
	 * @return array{order:int,address:string,state:string,counted_sats:int,depth:int,txids:string[]}
	 * @throws Refusal 'Notice' when the bytes are not a notice, 'Signature' when the signature does not match.
	 */
	public static function open( string $frame, string $public ): array {
		if ( strlen( $frame ) <= SODIUM_CRYPTO_SIGN_BYTES || SODIUM_CRYPTO_SIGN_PUBLICKEYBYTES !== strlen( $public ) ) {
			throw new Refusal( 'Notice' );
		}
		$body      = substr( $frame, 0, -SODIUM_CRYPTO_SIGN_BYTES );
		$signature = substr( $frame, -SODIUM_CRYPTO_SIGN_BYTES );
		$notice    = self::read_body( $body );
		if ( ! sodium_crypto_sign_verify_detached( $signature, $body, $public ) ) {
			throw new Refusal( 'Signature' );
		}
		return $notice;
	}

	/**
	 * @return array{order:int,address:string,state:string,counted_sats:int,depth:int,txids:string[]}
	 * @throws Refusal When the bytes are not a notice.
	 */
	private static function read_body( string $body ): array {
		try {
			$reader = new Reader( $body );
			if ( self::MAGIC !== $reader->take( 4 ) || self::VERSION !== $reader->u8() ) {
				throw new Refusal( 'Notice' );
			}
			$order   = $reader->u64();
			$address = $reader->take( $reader->u8() );
			$state   = $reader->u8();
			$counted = $reader->u64();
			$depth   = $reader->u32();
			$count   = $reader->u8();
			$txids   = array();
			for ( $i = 0; $i < $count; $i++ ) {
				$txids[] = bin2hex( $reader->take( 32 ) );
			}
			if ( ! $reader->is_empty() ) {
				throw new Refusal( 'Notice' );
			}
		} catch ( Refusal $e ) {
			throw new Refusal( 'Notice' );
		}
		if ( ! isset( self::STATES[ $state ] ) || ! Bech32::is_address( $address ) || 0 === $count || $count > self::MAX_TXIDS ) {
			throw new Refusal( 'Notice' );
		}
		return array(
			'order'        => $order,
			'address'      => $address,
			'state'        => self::STATES[ $state ],
			'counted_sats' => $counted,
			'depth'        => $depth,
			'txids'        => $txids,
		);
	}
}
