<?php
/**
 * A reader over bytes that never reads past the end.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Reader {

	/** @var string */
	private $bytes;

	/** @var int */
	private $at = 0;

	public function __construct( string $bytes ) {
		$this->bytes = $bytes;
	}

	/**
	 * The next $n bytes.
	 *
	 * @throws Refusal When fewer are left.
	 */
	public function take( int $n ): string {
		if ( $n < 0 || $n > strlen( $this->bytes ) - $this->at ) {
			throw new Refusal( 'Short' );
		}
		$out       = substr( $this->bytes, $this->at, $n );
		$this->at += $n;
		return $out;
	}

	/**
	 * One byte as a number.
	 */
	public function u8(): int {
		return ord( $this->take( 1 ) );
	}

	/**
	 * Four bytes, big endian.
	 */
	public function u32(): int {
		$value = unpack( 'N', $this->take( 4 ) );
		return (int) $value[1];
	}

	/**
	 * Eight bytes, big endian. A value above the largest PHP integer is
	 * refused.
	 *
	 * @throws Refusal When the number does not fit.
	 */
	public function u64(): int {
		$value = unpack( 'J', $this->take( 8 ) );
		if ( $value[1] < 0 ) {
			throw new Refusal( 'Number' );
		}
		return (int) $value[1];
	}

	/**
	 * Whether every byte has been read.
	 */
	public function is_empty(): bool {
		return strlen( $this->bytes ) === $this->at;
	}
}
