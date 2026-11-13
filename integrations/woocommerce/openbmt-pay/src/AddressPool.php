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

	const LOW = 20;

	/** @var int|null The index the next range must start at; null before the first. */
	private $next_index = null;

	/** @var array<int,array{0:int,1:string}> Unused addresses, as (index, address), in order. */
	private $queue = array();

	/** @var array<string,int> Address to the order that used it. */
	private $used = array();

	/**
	 * Adds a range that has been opened. It must start where the last ended;
	 * the first one starts anywhere. A range that is already held changes
	 * nothing and is "stale".
	 *
	 * @param int      $start     The index of the first address.
	 * @param string[] $addresses The addresses.
	 * @return string One of the constants of this class.
	 */
	public function add( int $start, array $addresses ): string {
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
			'next_index' => $this->next_index,
			'queue'      => $this->queue,
			'used'       => $this->used,
		);
	}

	/**
	 * A pool from plain data; anything that is not a pool as to_array makes
	 * it gives an empty one.
	 *
	 * @param mixed $data What was stored.
	 */
	public static function from_array( $data ): self {
		$pool = new self();
		if ( ! is_array( $data ) || ! isset( $data['queue'], $data['used'] ) || ! is_array( $data['queue'] ) || ! is_array( $data['used'] ) ) {
			return $pool;
		}
		$next = $data['next_index'] ?? null;
		if ( null !== $next && ! is_int( $next ) ) {
			return $pool;
		}
		foreach ( $data['queue'] as $entry ) {
			if ( ! is_array( $entry ) || 2 !== count( $entry ) || ! is_int( $entry[0] ?? null ) || ! is_string( $entry[1] ?? null ) ) {
				return new self();
			}
			$pool->queue[] = array( $entry[0], $entry[1] );
		}
		foreach ( $data['used'] as $address => $order ) {
			if ( ! is_int( $order ) ) {
				return new self();
			}
			$pool->used[ (string) $address ] = $order;
		}
		$pool->next_index = $next;
		return $pool;
	}
}
