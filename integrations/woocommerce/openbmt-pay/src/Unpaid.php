<?php
/**
 * WO-11: the cap on unpaid orders. Each unpaid order holds an address of the
 * pool, so a script that opens orders it never pays could use up the
 * addresses. Two limits hide Bitcoin at checkout. The open limit is the
 * owner's setting, and counts the unpaid orders whose quote is still open.
 * The day limit is ten times that, and counts the unpaid orders made in the
 * last day. An order counts when it holds an address and is not paid. Nothing
 * of the customer is read or kept for it.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Unpaid {

	/** The number of unpaid orders with an open quote that a store allows, unless the owner sets another. */
	const DEFAULT_MAX = 20;

	/** The least the owner may set. */
	const MIN = 1;

	/** The most the owner may set. */
	const MAX = 100;

	/** The seconds an order counts for in the day limit: a day. */
	const WINDOW_SECS = 86400;

	/** The day limit is this many times the owner's setting. */
	const DAY_FACTOR = 10;

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
	 * How many of the orders count for the day limit: made in the last day,
	 * with an address, and not paid.
	 *
	 * @param array<int,array{created:int,address:string,paid:bool,expires:int}> $orders The orders paid by this gateway.
	 * @param int                                                                 $now    The time now, in Unix seconds.
	 */
	public static function day_count( array $orders, int $now ): int {
		$count = 0;
		foreach ( $orders as $order ) {
			if ( '' !== $order['address'] && ! $order['paid'] && $order['created'] > $now - self::WINDOW_SECS ) {
				++$count;
			}
		}
		return $count;
	}

	/**
	 * How many of the orders count for the open limit: with an address, not
	 * paid, and a quote that runs out after now. An order with no expiry
	 * has expires 0, so it is not open.
	 *
	 * @param array<int,array{created:int,address:string,paid:bool,expires:int}> $orders The orders paid by this gateway.
	 * @param int                                                                 $now    The time now, in Unix seconds.
	 */
	public static function open_count( array $orders, int $now ): int {
		$count = 0;
		foreach ( $orders as $order ) {
			if ( '' !== $order['address'] && ! $order['paid'] && $order['expires'] > $now ) {
				++$count;
			}
		}
		return $count;
	}

	/**
	 * Whether no more address is given: as many orders have an open quote as
	 * the store allows, or ten times as many were left unpaid in a day.
	 */
	public static function at_cap( int $open, int $day, int $max ): bool {
		return $open >= $max || $day >= $max * self::DAY_FACTOR;
	}
}
