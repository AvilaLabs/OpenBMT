<?php
/**
 * WO-11: the cap on unpaid orders. The number the owner sets has bounds of
 * 1 and 100 and a default of 20; open quotes count against that number,
 * unpaid orders of the last 24 hours against ten times it; at either no more
 * address is given.
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\Unpaid;

$now = 1800000000;

// The option's bounds.
t_eq( 20, Unpaid::DEFAULT_MAX, 'wo_11 the default is 20' );
t_eq( 1, Unpaid::MIN, 'wo_11 the least is 1' );
t_eq( 100, Unpaid::MAX, 'wo_11 the most is 100' );
t_eq( 20, Unpaid::limit( null ), 'wo_11 no setting is the default' );
t_eq( 20, Unpaid::limit( '' ), 'wo_11 an empty setting is the default' );
t_eq( 20, Unpaid::limit( 'many' ), 'wo_11 a setting that is not a number is the default' );
t_eq( 20, Unpaid::limit( '1.5' ), 'wo_11 a setting that is not whole is the default' );
t_eq( 20, Unpaid::limit( '-3' ), 'wo_11 a negative setting is the default' );
t_eq( 20, Unpaid::limit( 2.0 ), 'wo_11 a float setting is the default' );
t_eq( 1, Unpaid::limit( '1' ), 'wo_11 1 is kept' );
t_eq( 1, Unpaid::limit( '0' ), 'wo_11 0 is raised to 1' );
t_eq( 1, Unpaid::limit( 0 ), 'wo_11 0 as a number is raised to 1' );
t_eq( 100, Unpaid::limit( '100' ), 'wo_11 100 is kept' );
t_eq( 100, Unpaid::limit( '101' ), 'wo_11 101 is lowered to 100' );
t_eq( 100, Unpaid::limit( '999999999' ), 'wo_11 a very large setting is lowered to 100' );
t_eq( 20, Unpaid::limit( '99999999999999999999' ), 'wo_11 a setting too long to be a number is the default' );
t_eq( 37, Unpaid::limit( ' 37 ' ), 'wo_11 spaces around a number are dropped' );
t_eq( 50, Unpaid::limit( 50 ), 'wo_11 a number is kept' );

// What counts.
$row = static function ( int $created, string $address = 'a0', bool $paid = false, int $expires = 0 ): array {
	return array( 'created' => $created, 'address' => $address, 'paid' => $paid, 'expires' => $expires );
};
t_eq( 10, Unpaid::DAY_FACTOR, 'wo_11 the day limit is ten times the setting' );
t_eq( 0, Unpaid::day_count( array(), $now ), 'wo_11 no orders, none unpaid' );
t_eq( 2, Unpaid::day_count( array( $row( $now - 10 ), $row( $now - 3600 ) ), $now ), 'wo_11 recent unpaid orders count in the day' );
t_eq( 0, Unpaid::day_count( array( $row( $now - 10, 'a0', true ) ), $now ), 'wo_11 a paid order does not count in the day' );
t_eq( 0, Unpaid::day_count( array( $row( $now - 86400 ), $row( $now - 90000 ) ), $now ), 'wo_11 an order of 24 hours ago or older does not count in the day' );
t_eq( 1, Unpaid::day_count( array( $row( $now - 86399 ) ), $now ), 'wo_11 an order just inside the 24 hours counts in the day' );
t_eq( 0, Unpaid::day_count( array( $row( $now - 10, '' ) ), $now ), 'wo_11 an order with no address does not count in the day' );

// Open quotes.
t_eq( 0, Unpaid::open_count( array(), $now ), 'wo_11 no orders, none open' );
t_eq( 0, Unpaid::open_count( array( $row( $now - 10, 'a0', false, $now ) ), $now ), 'wo_11 a quote that runs out now is not open' );
t_eq( 1, Unpaid::open_count( array( $row( $now - 10, 'a0', false, $now + 1 ) ), $now ), 'wo_11 a quote that runs out a second later is open' );
t_eq( 0, Unpaid::open_count( array( $row( $now - 10, 'a0', true, $now + 600 ) ), $now ), 'wo_11 a paid order is not open' );
t_eq( 0, Unpaid::open_count( array( $row( $now - 10, '', false, $now + 600 ) ), $now ), 'wo_11 an order with no address is not open' );
t_eq( 0, Unpaid::open_count( array( $row( $now - 10 ) ), $now ), 'wo_11 an order with no expiry is not open' );
t_eq( 1, Unpaid::open_count( array( $row( $now - 100000, 'a0', false, $now + 5 ) ), $now ), 'wo_11 an open quote counts whatever the age of the order' );

// The cap.
t_ok( ! Unpaid::at_cap( 19, 19, 20 ), 'wo_11 under the cap' );
t_ok( Unpaid::at_cap( 20, 20, 20 ), 'wo_11 open at the cap hides Bitcoin' );
t_ok( Unpaid::at_cap( 21, 21, 20 ), 'wo_11 over the cap' );
t_ok( Unpaid::at_cap( 1, 1, 1 ), 'wo_11 a cap of 1 is reached by one order' );
t_ok( Unpaid::at_cap( 0, 200, 20 ), 'wo_11 ten times the setting in a day hides Bitcoin' );
t_ok( ! Unpaid::at_cap( 19, 199, 20 ), 'wo_11 one under each limit does not' );
t_ok( ! Unpaid::at_cap( 0, 199, 20 ), 'wo_11 the day count under ten times the setting does not' );
t_ok( ! Unpaid::at_cap( 0, 20, 20 ), 'wo_11 20 in a day with no quote open does not hide Bitcoin' );
t_ok( Unpaid::at_cap( 0, 1000, 100 ), 'wo_11 a setting of 100 has a day limit of 1000' );
t_ok( ! Unpaid::at_cap( 0, 999, 100 ), 'wo_11 999 in a day is under it' );
t_eq( 1001, Unpaid::MAX * Unpaid::DAY_FACTOR + 1, 'wo_11 the store reads one more than the largest day limit' );

t_done();
