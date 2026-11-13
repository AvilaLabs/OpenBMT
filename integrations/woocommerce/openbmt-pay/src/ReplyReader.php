<?php
/**
 * RT-10 to RT-17: the reading of a source's reply into the text of its price,
 * the port of `parse_response` of `crates/openbmt-rates`. The reply is read
 * whole by Json, then only the place its source's shape names is looked at.
 * A refusal has the name of the Rust refusal; one of the price text is
 * named `Price:` and the Rust name of the price refusal.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class ReplyReader {

	/** The most bytes of a reply that are read (RT-10). */
	const MAX_BYTES = 65536;

	/** The most levels of arrays and objects (RT-11). */
	const MAX_DEPTH = 16;

	/**
	 * RT-10 to RT-17: the price in minor units per bitcoin in a reply.
	 *
	 * @param array{0:string,1:string,2:string,3:string,4:array<string,mixed>} $source The row of Rates::TABLE.
	 * @param string                                                          $body   The reply.
	 * @throws Refusal With the reason of the refusal.
	 */
	public static function minor( array $source, string $body ): int {
		$text = self::price_text( $source, $body );
		try {
			return Price::parse_minor( $text, $source[2] );
		} catch ( Refusal $e ) {
			throw new Refusal( 'Price:' . $e->reason );
		}
	}

	/**
	 * RT-10 to RT-17: the text of the price in a reply.
	 *
	 * @param array{0:string,1:string,2:string,3:string,4:array<string,mixed>} $source The row of Rates::TABLE.
	 * @param string                                                          $body   The reply.
	 * @throws Refusal With the reason of the refusal.
	 */
	public static function price_text( array $source, string $body ): string {
		$doc   = Json::parse( $body, self::MAX_BYTES, self::MAX_DEPTH );
		$shape = $source[4];
		switch ( $shape['kind'] ) {
			case 'kraken':
				return self::kraken( $doc );
			case 'coinbase':
				$data = self::field( $doc, 'data' );
				if ( self::text( $data, 'currency' ) !== $source[2] ) {
					throw new Refusal( 'WrongCurrency' );
				}
				return self::text( $data, 'amount' );
			case 'last':
				return self::text( $doc, 'last' );
			case 'text':
				self::checked( $doc, $shape['check'] );
				$found = self::walk( $doc, $shape['price'] )[0];
				if ( 'str' !== $found['t'] ) {
					throw new Refusal( 'WrongShape' );
				}
				return $found['v'];
			default:
				self::checked( $doc, $shape['check'] );
				list( $found, $escaped ) = self::walk( $doc, $shape['price'] );
				if ( 'num' !== $found['t'] ) {
					throw new Refusal( 'WrongShape' );
				}
				// A key spelled with an escape is not matched, and a number that does not start with a digit is refused.
				if ( $escaped || 1 !== preg_match( '/^[0-9]/', $found['v'] ) ) {
					throw new Refusal( 'MissingField' );
				}
				return $found['v'];
		}
	}

	/**
	 * RT-15, RT-16: Kraken's error array, the one entry of result, its c.
	 *
	 * @param array{t:string,v:mixed} $doc The document.
	 */
	private static function kraken( array $doc ): string {
		$errors = self::field( $doc, 'error' );
		if ( 'arr' !== $errors['t'] ) {
			throw new Refusal( 'WrongShape' );
		}
		if ( array() !== $errors['v'] ) {
			throw new Refusal( 'SourceError' );
		}
		$result = self::field( $doc, 'result' );
		if ( 'obj' !== $result['t'] || 1 !== count( $result['v'] ) ) {
			throw new Refusal( 'WrongShape' );
		}
		$last = self::field( $result['v'][0][1], 'c' );
		if ( 'arr' !== $last['t'] || array() === $last['v'] || 'str' !== $last['v'][0]['t'] ) {
			throw new Refusal( 'WrongShape' );
		}
		return $last['v'][0]['v'];
	}

	/**
	 * The value of a key of an object.
	 *
	 * @param array{t:string,v:mixed} $value An object.
	 * @return array{t:string,v:mixed}
	 * @throws Refusal 'WrongShape' for a value that is not an object, 'MissingField' for a key that is not there.
	 */
	private static function field( array $value, string $key ): array {
		if ( 'obj' !== $value['t'] ) {
			throw new Refusal( 'WrongShape' );
		}
		$found = Json::get( $value, $key );
		if ( null === $found ) {
			throw new Refusal( 'MissingField' );
		}
		return $found;
	}

	/**
	 * The text at a key of an object.
	 *
	 * @param array{t:string,v:mixed} $value An object.
	 * @throws Refusal When it is not there or not a string.
	 */
	private static function text( array $value, string $key ): string {
		$found = self::field( $value, $key );
		if ( 'str' !== $found['t'] ) {
			throw new Refusal( 'WrongShape' );
		}
		return $found['v'];
	}

	/**
	 * The value at a path of keys (strings) and first items (0), and whether
	 * a key on the way was written with an escape.
	 *
	 * @param array{t:string,v:mixed} $doc  The document.
	 * @param array<int,int|string>   $path The steps.
	 * @return array{0:array{t:string,v:mixed},1:bool}
	 * @throws Refusal 'WrongShape' or 'MissingField'.
	 */
	private static function walk( array $doc, array $path ): array {
		$escaped = false;
		$at      = $doc;
		foreach ( $path as $step ) {
			if ( is_int( $step ) ) {
				if ( 'arr' !== $at['t'] ) {
					throw new Refusal( 'WrongShape' );
				}
				if ( array() === $at['v'] ) {
					throw new Refusal( 'MissingField' );
				}
				$at = $at['v'][0];
				continue;
			}
			if ( 'obj' !== $at['t'] ) {
				throw new Refusal( 'WrongShape' );
			}
			$member = null;
			foreach ( $at['v'] as $m ) {
				if ( $m[0] === $step ) {
					$member = $m;
					break;
				}
			}
			if ( null === $member ) {
				throw new Refusal( 'MissingField' );
			}
			$escaped = $escaped || $member[2];
			$at      = $member[1];
		}
		return array( $at, $escaped );
	}

	/**
	 * RT-17: the field of a check says what it must.
	 *
	 * @param array{t:string,v:mixed}|null $doc   The document.
	 * @param array<string,mixed>|null     $check The check of the shape.
	 */
	private static function checked( array $doc, ?array $check ): void {
		if ( null === $check ) {
			return;
		}
		$said = self::walk( $doc, $check['at'] )[0];
		if ( 'str' !== $said['t'] ) {
			throw new Refusal( 'WrongShape' );
		}
		if ( $said['v'] !== $check['equals'] ) {
			throw new Refusal( 'Market' === $check['fault'] ? 'WrongCurrency' : 'SourceError' );
		}
	}
}
