<?php
/**
 * WO-10, WO-11: the use-once pool of addresses.
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\AddressPool;

$v    = t_vectors( 'web_order.json' );
$list = $v['range']['addresses'];

function t_fake_addresses( int $from, int $count ): array {
	$out = array();
	for ( $i = $from; $i < $from + $count; $i++ ) {
		$out[] = 'a' . $i;
	}
	return $out;
}

// WO-10: the first range starts anywhere, the next where the last ended.
$pool = new AddressPool();
t_eq( null, $pool->next_index(), 'wo_10 a new pool has no index' );
t_eq( AddressPool::ACCEPTED, $pool->add( 7, $list ), 'wo_10 the first range is accepted' );
t_eq( 12, $pool->next_index(), 'wo_10 the next range starts after it' );
t_eq( 5, $pool->remaining(), 'wo_10 five are left' );
t_eq( AddressPool::STALE, $pool->add( 7, $list ), 'wo_10 the same range again changes nothing' );
t_eq( AddressPool::GAP, $pool->add( 13, t_fake_addresses( 0, 2 ) ), 'wo_10 a range that skips is refused' );
t_eq( AddressPool::OVERLAP, $pool->add( 10, t_fake_addresses( 0, 5 ) ), 'wo_10 a range that overlaps is refused' );
t_eq( AddressPool::DUPLICATE, $pool->add( 12, array( 'x1', 'x1' ) ), 'wo_10 an address twice in a range is refused' );
t_eq( AddressPool::DUPLICATE, $pool->add( 12, array( 'x1', $list[0] ) ), 'wo_10 an address held is refused' );
t_eq( 5, $pool->remaining(), 'wo_10 refusals change nothing' );
t_eq( 12, $pool->next_index(), 'wo_10 refusals keep the index' );

// WO-10: used once, in order.
$taken = array();
for ( $i = 0; $i < 5; $i++ ) {
	$taken[] = $pool->take( 100 + $i );
}
t_eq( array( 7, $list[0] ), $taken[0], 'wo_10 the first address first' );
t_eq( array( 11, $list[4] ), $taken[4], 'wo_10 the last address last' );
t_eq( null, $pool->take( 105 ), 'wo_11 none left gives none' );
t_ok( $pool->is_empty(), 'wo_11 the pool is empty' );
t_eq( 102, $pool->order_of( $list[2] ), 'wo_10 the order that used an address' );
t_eq( null, $pool->order_of( 'a' ), 'wo_10 an address not used has no order' );
t_eq( AddressPool::DUPLICATE, $pool->add( 12, array( $list[0] ) ), 'wo_10 an address used is never given again' );
t_eq( AddressPool::ACCEPTED, $pool->add( 12, t_fake_addresses( 0, 3 ) ), 'wo_10 the next range after the last is accepted' );

// WO-11: running low.
$pool = new AddressPool();
t_ok( $pool->is_low(), 'wo_11 an empty pool is low' );
$pool->add( 0, t_fake_addresses( 0, 100 ) );
t_ok( ! $pool->is_low(), 'wo_11 one hundred is not low' );
for ( $i = 0; $i < 80; $i++ ) {
	$pool->take( $i );
}
t_eq( 20, $pool->remaining(), 'wo_11 twenty left' );
t_ok( ! $pool->is_low(), 'wo_11 twenty is not low' );
$pool->take( 80 );
t_ok( $pool->is_low(), 'wo_11 nineteen is low' );
t_ok( ! $pool->is_empty(), 'wo_11 nineteen is not empty' );

// Never twice: every address of a hundred is handed out exactly once.
$pool = new AddressPool();
$pool->add( 0, t_fake_addresses( 0, 100 ) );
$seen = array();
while ( null !== ( $entry = $pool->take( count( $seen ) + 1 ) ) ) {
	t_ok( ! isset( $seen[ $entry[1] ] ), 'wo_11 an address is not given twice' );
	$seen[ $entry[1] ] = true;
}
t_eq( 100, count( $seen ), 'wo_11 all one hundred were given' );

// The storage round trip, and refusal of damaged data.
$pool = new AddressPool();
$pool->add( 4, t_fake_addresses( 0, 6 ) );
$pool->take( 9 );
$copy = AddressPool::from_array( json_decode( json_encode( $pool->to_array() ), true ) );
t_eq( $pool->to_array(), $copy->to_array(), 'wo_10 the pool survives storage' );
t_eq( 5, $copy->remaining(), 'wo_10 the stored pool has five left' );
t_eq( 9, $copy->order_of( 'a0' ), 'wo_10 the stored pool knows the used address' );
foreach ( array( null, 'x', array(), array( 'queue' => 1, 'used' => array() ), array( 'queue' => array( array( 'a' ) ), 'used' => array() ), array( 'queue' => array(), 'used' => array( 'a' => 'b' ) ) ) as $i => $bad ) {
	t_eq( 0, AddressPool::from_array( $bad )->remaining(), "wo_10 damaged data $i gives an empty pool" );
}
t_done();
