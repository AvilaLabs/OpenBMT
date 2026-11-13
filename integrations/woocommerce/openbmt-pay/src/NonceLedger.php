<?php
/**
 * The numbers used once of WO-20.
 */

declare(strict_types=1);

namespace OpenBmtPay;

/**
 * The numbers used once that have been seen within the time window. A number
 * is kept until its request's time is out of the window, after which the time
 * check refuses the request on its own.
 */
final class NonceLedger {

	/** The most numbers kept; a full ledger refuses (fail closed). */
	const MAX = 2000;

	/** @var array<string,int> Number in hex to its request's time. */
	private $seen = array();

	/**
	 * Takes a number. A number seen before is refused.
	 *
	 * @throws Refusal 'Replay' or 'Full'.
	 */
	public function use_once( string $nonce_hex, int $time, int $now ): void {
		foreach ( $this->seen as $nonce => $at ) {
			if ( $at < $now - RequestChecker::WINDOW_SECS ) {
				unset( $this->seen[ $nonce ] );
			}
		}
		if ( isset( $this->seen[ $nonce_hex ] ) ) {
			throw new Refusal( 'Replay' );
		}
		if ( count( $this->seen ) >= self::MAX ) {
			throw new Refusal( 'Full' );
		}
		$this->seen[ $nonce_hex ] = $time;
	}

	/**
	 * The numbers as plain data.
	 *
	 * @return array<string,int>
	 */
	public function to_array(): array {
		return $this->seen;
	}

	/**
	 * A ledger from plain data. Anything that is not what to_array makes is
	 * damaged and gives null.
	 *
	 * @param mixed $data What was stored.
	 */
	public static function from_array( $data ): ?self {
		if ( ! is_array( $data ) || count( $data ) > self::MAX ) {
			return null;
		}
		$ledger = new self();
		foreach ( $data as $nonce => $at ) {
			if ( ! is_int( $at ) || 1 !== preg_match( '/^[0-9a-f]{32}$/', (string) $nonce ) ) {
				return null;
			}
			$ledger->seen[ (string) $nonce ] = $at;
		}
		return $ledger;
	}
}
