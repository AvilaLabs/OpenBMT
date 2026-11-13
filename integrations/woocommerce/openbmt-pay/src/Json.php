<?php
/**
 * A strict, small JSON reader, the port of `crates/openbmt-json`: the grammar
 * of RFC 8259 checked on the whole document, bounds on size and depth, a key
 * twice in one object refused however it was spelled, and every number kept
 * as its exact text. It is the only reader of a rate source's reply; no
 * PHP function that decodes JSON is used on one.
 *
 * A value is an array with `t`, its kind, and `v`: 'null' and 'bool' (v true
 * or false), 'num' (v the number's text), 'str' (v the text), 'arr' (v the
 * items) and 'obj' (v the members, each as array of key, value and whether
 * the key was written with an escape).
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Json {

	/** @var string */
	private $bytes;

	/** @var int */
	private $length;

	/** @var int */
	private $pos = 0;

	/** @var int */
	private $values = 0;

	/** @var int */
	private $max_depth;

	/** @var int */
	private $max_values;

	private function __construct( string $bytes, int $max_depth, int $max_values ) {
		$this->bytes      = $bytes;
		$this->length     = strlen( $bytes );
		$this->max_depth  = $max_depth;
		$this->max_values = $max_values;
	}

	/**
	 * Reads a whole document. The length is checked first, then that the bytes
	 * are UTF-8, then the document, then that nothing follows it.
	 *
	 * @return array{t:string,v:mixed}
	 * @throws Refusal 'TooLong', 'NotUtf8', 'TooDeep', 'Malformed', 'TrailingBytes' or 'DuplicateKey'.
	 */
	public static function parse( string $bytes, int $max_bytes, int $max_depth, int $max_values = 400000 ): array {
		if ( strlen( $bytes ) > $max_bytes ) {
			throw new Refusal( 'TooLong' );
		}
		if ( 1 !== preg_match( '//u', $bytes ) ) {
			throw new Refusal( 'NotUtf8' );
		}
		$reader = new self( $bytes, $max_depth, $max_values );
		$value  = $reader->value( 0 );
		$reader->skip_space();
		if ( $reader->pos !== $reader->length ) {
			throw new Refusal( 'TrailingBytes' );
		}
		return $value;
	}

	/**
	 * The value of a key of an object, or null when this is not an object or
	 * has no such key.
	 *
	 * @param array{t:string,v:mixed} $object A value.
	 * @return array{t:string,v:mixed}|null
	 */
	public static function get( array $object, string $key ): ?array {
		if ( 'obj' !== $object['t'] ) {
			return null;
		}
		foreach ( $object['v'] as $member ) {
			if ( $member[0] === $key ) {
				return $member[1];
			}
		}
		return null;
	}

	private function peek(): ?string {
		return $this->pos < $this->length ? $this->bytes[ $this->pos ] : null;
	}

	private function skip_space(): void {
		while ( $this->pos < $this->length && ( ' ' === $this->bytes[ $this->pos ] || "\t" === $this->bytes[ $this->pos ] || "\n" === $this->bytes[ $this->pos ] || "\r" === $this->bytes[ $this->pos ] ) ) {
			++$this->pos;
		}
	}

	private function expect( string $byte ): void {
		if ( $this->peek() !== $byte ) {
			throw new Refusal( 'Malformed' );
		}
		++$this->pos;
	}

	/**
	 * @param array{t:string,v:mixed} $value What the word is.
	 * @return array{t:string,v:mixed}
	 */
	private function word( string $word, array $value ): array {
		if ( substr( $this->bytes, $this->pos, strlen( $word ) ) !== $word ) {
			throw new Refusal( 'Malformed' );
		}
		$this->pos += strlen( $word );
		return $value;
	}

	/**
	 * @return array{t:string,v:mixed}
	 */
	private function value( int $depth ): array {
		$this->skip_space();
		++$this->values;
		if ( $this->values > $this->max_values ) {
			throw new Refusal( 'TooLong' );
		}
		$c = $this->peek();
		if ( '{' === $c ) {
			return $this->object( $depth + 1 );
		}
		if ( '[' === $c ) {
			return $this->arr( $depth + 1 );
		}
		if ( '"' === $c ) {
			return array(
				't' => 'str',
				'v' => $this->string()[0],
			);
		}
		if ( 't' === $c ) {
			return $this->word(
				'true',
				array(
					't' => 'bool',
					'v' => true,
				)
			);
		}
		if ( 'f' === $c ) {
			return $this->word(
				'false',
				array(
					't' => 'bool',
					'v' => false,
				)
			);
		}
		if ( 'n' === $c ) {
			return $this->word(
				'null',
				array(
					't' => 'null',
					'v' => null,
				)
			);
		}
		if ( '-' === $c || ( null !== $c && $c >= '0' && $c <= '9' ) ) {
			return $this->number();
		}
		throw new Refusal( 'Malformed' );
	}

	/**
	 * @return array{t:string,v:mixed}
	 */
	private function arr( int $depth ): array {
		if ( $depth > $this->max_depth ) {
			throw new Refusal( 'TooDeep' );
		}
		$this->expect( '[' );
		$items = array();
		$this->skip_space();
		if ( ']' === $this->peek() ) {
			++$this->pos;
			return array(
				't' => 'arr',
				'v' => $items,
			);
		}
		while ( true ) {
			$items[] = $this->value( $depth );
			$this->skip_space();
			$c = $this->peek();
			if ( ',' === $c ) {
				++$this->pos;
			} elseif ( ']' === $c ) {
				++$this->pos;
				return array(
					't' => 'arr',
					'v' => $items,
				);
			} else {
				throw new Refusal( 'Malformed' );
			}
		}
	}

	/**
	 * @return array{t:string,v:mixed}
	 */
	private function object( int $depth ): array {
		if ( $depth > $this->max_depth ) {
			throw new Refusal( 'TooDeep' );
		}
		$this->expect( '{' );
		$items = array();
		$this->skip_space();
		if ( '}' === $this->peek() ) {
			++$this->pos;
			return array(
				't' => 'obj',
				'v' => $items,
			);
		}
		while ( true ) {
			$this->skip_space();
			$key = $this->string();
			$this->skip_space();
			$this->expect( ':' );
			$value   = $this->value( $depth );
			$items[] = array( $key[0], $value, $key[1] );
			$this->skip_space();
			$c = $this->peek();
			if ( ',' === $c ) {
				++$this->pos;
			} elseif ( '}' === $c ) {
				++$this->pos;
				break;
			} else {
				throw new Refusal( 'Malformed' );
			}
		}
		$seen = array();
		foreach ( $items as $member ) {
			if ( isset( $seen[ $member[0] ] ) ) {
				throw new Refusal( 'DuplicateKey' );
			}
			$seen[ $member[0] ] = true;
		}
		return array(
			't' => 'obj',
			'v' => $items,
		);
	}

	private function hex4(): int {
		$digits = substr( $this->bytes, $this->pos, 4 );
		if ( 1 !== preg_match( '/^[0-9a-fA-F]{4}$/', $digits ) ) {
			throw new Refusal( 'Malformed' );
		}
		$this->pos += 4;
		return (int) hexdec( $digits );
	}

	/**
	 * A string with its escapes resolved, and whether it had any.
	 *
	 * @return array{0:string,1:bool}
	 */
	private function string(): array {
		$this->expect( '"' );
		$out     = '';
		$escaped = false;
		while ( true ) {
			if ( $this->pos >= $this->length ) {
				throw new Refusal( 'Malformed' );
			}
			$b = $this->bytes[ $this->pos ];
			++$this->pos;
			if ( '"' === $b ) {
				break;
			}
			if ( ord( $b ) < 0x20 ) {
				throw new Refusal( 'Malformed' );
			}
			if ( '\\' !== $b ) {
				$out .= $b;
				continue;
			}
			$escaped = true;
			if ( $this->pos >= $this->length ) {
				throw new Refusal( 'Malformed' );
			}
			$e = $this->bytes[ $this->pos ];
			++$this->pos;
			switch ( $e ) {
				case '"':
					$out .= '"';
					break;
				case '\\':
					$out .= '\\';
					break;
				case '/':
					$out .= '/';
					break;
				case 'b':
					$out .= "\x08";
					break;
				case 'f':
					$out .= "\x0c";
					break;
				case 'n':
					$out .= "\n";
					break;
				case 'r':
					$out .= "\r";
					break;
				case 't':
					$out .= "\t";
					break;
				case 'u':
					$out .= self::utf8( $this->unicode() );
					break;
				default:
					throw new Refusal( 'Malformed' );
			}
		}
		return array( $out, $escaped );
	}

	/**
	 * The code point of a \u escape with the four digits still to read; a high
	 * surrogate must be followed by a low one, and a low one alone is refused.
	 */
	private function unicode(): int {
		$first = $this->hex4();
		if ( $first >= 0xd800 && $first < 0xdc00 ) {
			$this->expect( '\\' );
			$this->expect( 'u' );
			$second = $this->hex4();
			if ( $second < 0xdc00 || $second >= 0xe000 ) {
				throw new Refusal( 'Malformed' );
			}
			return 0x10000 + ( ( $first - 0xd800 ) << 10 ) + ( $second - 0xdc00 );
		}
		if ( $first >= 0xdc00 && $first < 0xe000 ) {
			throw new Refusal( 'Malformed' );
		}
		return $first;
	}

	private static function utf8( int $c ): string {
		if ( $c < 0x80 ) {
			return chr( $c );
		}
		if ( $c < 0x800 ) {
			return chr( 0xc0 | ( $c >> 6 ) ) . chr( 0x80 | ( $c & 0x3f ) );
		}
		if ( $c < 0x10000 ) {
			return chr( 0xe0 | ( $c >> 12 ) ) . chr( 0x80 | ( ( $c >> 6 ) & 0x3f ) ) . chr( 0x80 | ( $c & 0x3f ) );
		}
		return chr( 0xf0 | ( $c >> 18 ) ) . chr( 0x80 | ( ( $c >> 12 ) & 0x3f ) ) . chr( 0x80 | ( ( $c >> 6 ) & 0x3f ) ) . chr( 0x80 | ( $c & 0x3f ) );
	}

	private function digits(): int {
		$start = $this->pos;
		while ( $this->pos < $this->length && $this->bytes[ $this->pos ] >= '0' && $this->bytes[ $this->pos ] <= '9' ) {
			++$this->pos;
		}
		return $this->pos - $start;
	}

	/**
	 * The JSON number grammar; the number is kept as its exact text.
	 *
	 * @return array{t:string,v:mixed}
	 */
	private function number(): array {
		$start = $this->pos;
		if ( '-' === $this->peek() ) {
			++$this->pos;
		}
		$c = $this->peek();
		if ( '0' === $c ) {
			++$this->pos;
		} elseif ( null !== $c && $c >= '1' && $c <= '9' ) {
			$this->digits();
		} else {
			throw new Refusal( 'Malformed' );
		}
		if ( '.' === $this->peek() ) {
			++$this->pos;
			if ( 0 === $this->digits() ) {
				throw new Refusal( 'Malformed' );
			}
		}
		$c = $this->peek();
		if ( 'e' === $c || 'E' === $c ) {
			++$this->pos;
			$c = $this->peek();
			if ( '+' === $c || '-' === $c ) {
				++$this->pos;
			}
			if ( 0 === $this->digits() ) {
				throw new Refusal( 'Malformed' );
			}
		}
		return array(
			't' => 'num',
			'v' => substr( $this->bytes, $start, $this->pos - $start ),
		);
	}
}
