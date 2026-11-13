<?php
/**
 * WO-05: the pairing code, read strictly. Bytes: the magic, the version, the
 * public key (32), the secret (32), the length of the shop's name (1), the
 * name, and the first 4 bytes of the SHA-256 of everything before. The text
 * is those bytes in upper-case base32 without padding.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Pairing {

	const MAGIC = 'OBM`';

	const VERSION = 1;

	const MAX_NAME_BYTES = 64;

	/** @var string */
	public $public;

	/** @var string */
	public $secret;

	/** @var string */
	public $shop_name;

	private function __construct( string $public, string $secret, string $shop_name ) {
		$this->public    = $public;
		$this->secret    = $secret;
		$this->shop_name = $shop_name;
	}

	/**
	 * Reads the text of a code. White space around it is ignored.
	 *
	 * @throws Refusal When the text is not a code.
	 */
	public static function from_text( string $text ): self {
		return self::from_bytes( Bytes::from_base32( trim( $text, " \t\r\n" ) ) );
	}

	/**
	 * Reads the bytes of a code.
	 *
	 * @throws Refusal When the bytes are not a code.
	 */
	public static function from_bytes( string $bytes ): self {
		if ( strlen( $bytes ) < 4 ) {
			throw new Refusal( 'PairingCode' );
		}
		$body  = substr( $bytes, 0, -4 );
		$check = substr( $bytes, -4 );
		if ( ! hash_equals( substr( hash( 'sha256', $body, true ), 0, 4 ), $check ) ) {
			throw new Refusal( 'PairingCode' );
		}
		try {
			$reader = new Reader( $body );
			if ( self::MAGIC !== $reader->take( 4 ) || self::VERSION !== $reader->u8() ) {
				throw new Refusal( 'PairingCode' );
			}
			$public = $reader->take( 32 );
			$secret = $reader->take( 32 );
			$name   = $reader->take( $reader->u8() );
			if ( ! $reader->is_empty() ) {
				throw new Refusal( 'PairingCode' );
			}
		} catch ( Refusal $e ) {
			throw new Refusal( 'PairingCode' );
		}
		if ( ! self::name_is_valid( $name ) ) {
			throw new Refusal( 'ShopName' );
		}
		return new self( $public, $secret, $name );
	}

	/**
	 * A name of 1 to 64 bytes of UTF-8 with no byte below 0x20 and no 0x7F.
	 */
	public static function name_is_valid( string $name ): bool {
		return '' !== $name
			&& strlen( $name ) <= self::MAX_NAME_BYTES
			&& 1 === preg_match( '//u', $name )
			&& 0 === preg_match( '/[\x00-\x1f\x7f]/', $name );
	}

	/**
	 * The fingerprint of a public key: the first 8 bytes of its SHA-256 in
	 * hex, in four groups of four with a dash between.
	 */
	public static function fingerprint_of( string $public ): string {
		$text = bin2hex( substr( hash( 'sha256', $public, true ), 0, 8 ) );
		return implode( '-', str_split( $text, 4 ) );
	}

	/**
	 * WO-10: the id of a pairing, the first 8 bytes of the SHA-256 of its
	 * secret. A range carries it, signed.
	 */
	public static function id_of( string $secret ): string {
		return substr( hash( 'sha256', $secret, true ), 0, 8 );
	}

	/**
	 * The id of this pairing.
	 */
	public function id(): string {
		return self::id_of( $this->secret );
	}

	/**
	 * The fingerprint of the key of this code.
	 */
	public function fingerprint(): string {
		return self::fingerprint_of( $this->public );
	}
}
