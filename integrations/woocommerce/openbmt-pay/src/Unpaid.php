<?php
/**
 * WO-11: the cap on unpaid orders. Each unpaid order holds an address of the
 * pool, so a script that opens orders it never pays could use up the
 * addresses, and the room an owner's wallet looks in. The cap counts the
 * orders made in the last day that hold an address and are not paid. Nothing
 * of the customer is read or kept for it.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Unpaid {

	/** The number of unpaid orders a day that a store allows, unless the owner sets another. */
	const DEFAULT_MAX = 20;

	/** The least the owner may set. */
	const MIN = 1;

	/** The most the owner may set. */
	const MAX = 100;

	/** The seconds an order counts for: a day. */
	const WINDOW_SECS = 86400;

	/**
	 * The number the owner set, as a whole number from MIN to MAX. A value that
	 * is not a whole number is the default; one outside the bounds is the
	 * nearest bound.
	 *
	 * @param mixed $raw What was stored or typed.
	 */
	public static function limit( $raw ): int {
		if ( is_int( $raw ) ) {
			$n = $raw;
		} elseif ( is_string( $raw ) && 1 === preg_match( '/^\s*[0-9]{1,9}\s*$/', $raw ) ) {
			$n = (int) $raw;
		} else {
			return self::DEFAULT_MAX;
		}
		return max( self::MIN, min( self::MAX, $n ) );
	}

	/**
	 * How many of the orders count: made in the last day, with an address,
	 * and not paid.
	 *
	 * @param array<int,array{created:int,address:string,paid:bool}> $orders The orders paid by this gateway.
	 * @param int                                                     $now    The time now, in Unix seconds.
	 */
	public static function count( array $orders, int $now ): int {
		$count = 0;
		foreach ( $orders as $order ) {
			if ( '' !== $order['address'] && ! $order['paid'] && $order['created'] > $now - self::WINDOW_SECS ) {
				++$count;
			}
		}
		return $count;
	}

	/**
	 * Whether no more address is given: as many orders are unpaid as the
	 * store allows.
	 */
	public static function at_cap( int $count, int $max ): bool {
		return $count >= $max;
	}
}
