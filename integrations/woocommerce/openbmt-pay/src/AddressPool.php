<?php
/**
 * WO-10, WO-11: the addresses the owner phone gave, used once each and in
 * order. The pool is plain data so the storage can be anything: to_array
 * and from_array carry it.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class AddressPool {

	const ACCEPTED = 'accepted';

	const STALE = 'stale';

	const GAP = 'gap';

	const OVERLAP = 'overlap';

	const DUPLICATE = 'duplicate';

	const WRONG_PAIRING = 'wrong_pairing';

	const LOW = 20;

	/** @var string The id of the pairing the pool belongs to (8 bytes); only its ranges are taken. */
	private $pairing_id;

	/** @var int|null The index the next range must start at; null before the first. */
	private $next_index = null;

	/** @var array<int,array{0:int,1:string}> Unused addresses, as (index, address), in order. */
	private $queue = array();

	/** @var array<string,int> Address to the order that used it. */
	private $used = array();

	/**
	 * An empty pool for a pairing: it takes the first range of that pairing
	 * and no range of another.
	 *
	 * @param string $pairing_id The id of the pairing (8 bytes).
	 */
	public function __construct( string $pairing_id ) {
		$this->pairing_id = $pairing_id;
	}

	/**
	 * Adds a range that has been opened. It must start where the last ended;
	 * the first one starts anywhere. A range that is already held changes
	 * nothing and is "stale".
	 *
	 * @param int      $start     The index of the first address.
	 * @param string[] $addresses The addresses.
	 * @param string   $pairing_id The id of the pairing the range was signed for.
	 * @return string One of the constants of this class.
	 */
	public function add( int $start, array $addresses, string $pairing_id ): string {
		if ( ! hash_equals( $this->pairing_id, $pairing_id ) ) {
			return self::WRONG_PAIRING;
		}
		$count = count( $addresses );
		if ( null !== $this->next_index ) {
			if ( $start + $count <= $this->next_index ) {
				return self::STALE;
			}
			if ( $start > $this->next_index ) {
				return self::GAP;
			}
			if ( $start < $this->next_index ) {
				return self::OVERLAP;
			}
		}
		$held = array();
		foreach ( $this->queue as $entry ) {
			$held[ $entry[1] ] = true;
		}
		foreach ( $addresses as $address ) {
			if ( isset( $held[ $address ] ) || isset( $this->used[ $address ] ) ) {
				return self::DUPLICATE;
			}
			$held[ $address ] = true;
		}
		if ( count( $held ) !== count( $this->queue ) + $count ) {
			return self::DUPLICATE;
		}
		foreach ( $addresses as $i => $address ) {
			$this->queue[] = array( $start + $i, $address );
		}
		$this->next_index = $start + $count;
		return self::ACCEPTED;
	}

	/**
	 * Gives the next address to an order. The address is never given again,
	 * whatever happens to the order.
	 *
	 * @return array{0:int,1:string}|null The index and address, or null when none is left.
	 */
	public function take( int $order ): ?array {
		$entry = array_shift( $this->queue );
		if ( null === $entry ) {
			return null;
		}
		$this->used[ $entry[1] ] = $order;
		return $entry;
	}

	/**
	 * How many addresses are left.
	 */
	public function remaining(): int {
		return count( $this->queue );
	}

	/**
	 * WO-11: fewer than 20 left.
	 */
	public function is_low(): bool {
		return $this->remaining() < self::LOW;
	}

	/**
	 * WO-11: none left.
	 */
	public function is_empty(): bool {
		return 0 === $this->remaining();
	}

	/**
	 * The order that used an address, if any.
	 */
	public function order_of( string $address ): ?int {
		return $this->used[ $address ] ?? null;
	}

	/**
	 * The index the next range must start at.
	 */
	public function next_index(): ?int {
		return $this->next_index;
	}

	/**
	 * The pool as plain data.
	 *
	 * @return array<string,mixed>
	 */
	public function to_array(): array {
		return array(
			'pairing_id' => bin2hex( $this->pairing_id ),
			'next_index' => $this->next_index,
			'queue'      => $this->queue,
			'used'       => $this->used,
		);
	}

	/**
	 * A pool from plain data. Data that is not exactly what to_array makes is
	 * damaged and gives null: the pool is never guessed at.
	 *
	 * @param mixed $data What was stored.
	 */
	public static function from_array( $data ): ?self {
		if ( ! is_array( $data ) || array( 'next_index', 'pairing_id', 'queue', 'used' ) !== self::sorted_keys( $data ) ) {
			return null;
		}
		if ( ! is_string( $data['pairing_id'] ) || 1 !== preg_match( '/^[0-9a-f]{16}$/', $data['pairing_id'] ) ) {
			return null;
		}
		$next = $data['next_index'];
		if ( null !== $next && ( ! is_int( $next ) || $next < 0 || $next > 0xffffffff ) ) {
			return null;
		}
		if ( ! is_array( $data['queue'] ) || ! is_array( $data['used'] ) || ( null === $next && ( array() !== $data['queue'] || array() !== $data['used'] ) ) ) {
			return null;
		}
		$pool             = new self( (string) hex2bin( $data['pairing_id'] ) );
		$pool->next_index = $next;
		$seen             = array();
		$expect           = null;
		foreach ( $data['queue'] as $position => $entry ) {
			if ( ! is_int( $position ) || ! is_array( $entry ) || array( 0, 1 ) !== array_keys( $entry ) || ! is_int( $entry[0] ) || ! is_string( $entry[1] ) ) {
				return null;
			}
			$length = strlen( $entry[1] );
			if ( $length < 1 || $length > 90 || isset( $seen[ $entry[1] ] ) || ( null !== $expect && $entry[0] !== $expect ) ) {
				return null;
			}
			$seen[ $entry[1] ] = true;
			$expect            = $entry[0] + 1;
			$pool->queue[]     = array( $entry[0], $entry[1] );
		}
		if ( null !== $expect && $expect !== $next ) {
			return null;
		}
		foreach ( $data['used'] as $address => $order ) {
			if ( ! is_int( $order ) || $order < 0 || isset( $seen[ (string) $address ] ) ) {
				return null;
			}
			$pool->used[ (string) $address ] = $order;
		}
		return $pool;
	}

	/**
	 * The keys of an array in order, to compare with what a pool has.
	 *
	 * @param array<mixed> $data An array.
	 * @return array<int,int|string>
	 */
	private static function sorted_keys( array $data ): array {
		$keys = array_keys( $data );
		sort( $keys );
		return $keys;
	}
}
