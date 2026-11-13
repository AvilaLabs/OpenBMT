<?php
/**
 * WO-30 to WO-32, WO-50: pairing, a range, an order and a notice played
 * through the classes: a notice that is good, one with a wrong signature, a
 * wrong address, a repeated one, and an address never given twice.
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\AddressPool;
use OpenBmtPay\AddressRange;
use OpenBmtPay\Bytes;
use OpenBmtPay\Notice;
use OpenBmtPay\NoticeDecision;
use OpenBmtPay\Pairing;
use OpenBmtPay\Quote;

/**
 * A signed notice, made as the Rust side makes one.
 *
 * @param array<string,mixed> $n   The notice.
 * @param string              $key The secret key of the signature.
 */
function t_notice_frame( array $n, string $key ): string {
	$states = array_flip( Notice::STATES );
	$body   = Notice::MAGIC . chr( 1 ) . pack( 'J', $n['order'] ) . chr( strlen( $n['address'] ) ) . $n['address']
		. chr( $states[ $n['state'] ] ) . pack( 'J', $n['counted_sats'] ) . pack( 'N', $n['depth'] ) . chr( count( $n['txids'] ) );
	foreach ( $n['txids'] as $t ) {
		$body .= hex2bin( $t );
	}
	return $body . sodium_crypto_sign_detached( $body, $key );
}

$v      = t_vectors( 'web_order.json' );
$pair   = sodium_crypto_sign_seed_keypair( Bytes::from_hex( $v['key']['seed'] ) );
$secret = sodium_crypto_sign_secretkey( $pair );

// Pairing, a range, an order.
$code = Pairing::from_text( $v['pairing']['text'] );
$open = AddressRange::open( Bytes::from_hex( $v['range']['frame'] ), $code->public, $code->id() );
$pool = new AddressPool( $code->id() );
t_eq( AddressPool::ACCEPTED, $pool->add( $open['start'], $open['addresses'], $code->id() ), 'wo_10 the range of the pairing is accepted' );

$now   = 1800000000;
$quote = Quote::make( 'USD', 2500, array( array( 'kraken', $now, '73550.1' ), array( 'coinbase', $now, '73552.175' ) ), $now );
$taken = $pool->take( 1042 );
t_eq( $open['addresses'][0], $taken[1], 'wo_10 the order gets the first address' );
$second = $pool->take( 1043 );
t_ok( $second[1] !== $taken[1], 'wo_11 the next order gets another address' );

// The store's record of the orders.
$orders = array(
	1042 => array(
		'address'     => $taken[1],
		'paid'        => false,
		'last_notice' => null,
		'sats'        => $quote['sats'],
	),
	1043 => array(
		'address'     => $second[1],
		'paid'        => false,
		'last_notice' => null,
		'sats'        => $quote['sats'],
	),
);
$lookup = static function ( int $order ) use ( &$orders ): ?array {
	return $orders[ $order ] ?? null;
};
$tx1 = str_repeat( 'ab', 32 );
$tx2 = str_repeat( 'cd', 32 );
$make = static function ( int $order, string $address, string $state, int $sats, int $depth = 3, ?array $txids = null ) use ( $tx1, $tx2, $secret ): string {
	return t_notice_frame(
		array(
			'order'        => $order,
			'address'      => $address,
			'state'        => $state,
			'counted_sats' => $sats,
			'depth'        => $depth,
			'txids'        => $txids ?? array( $tx1, $tx2 ),
		),
		$secret
	);
};

// WO-31: paid completes with the first transaction ID.
$frame = $make( 1042, $taken[1], 'paid', $quote['sats'] );
$d     = NoticeDecision::decide( $frame, $code->public, $lookup );
t_eq( 'complete', $d['action'], 'wo_31 a paid notice completes the order' );
t_eq( $tx1, $d['transaction_id'], 'wo_31 with the first transaction ID' );
t_eq( $quote['sats'], $d['notice']['counted_sats'], 'wo_31 the note has the amount counted' );
t_eq( $quote['sats'], $d['asked_sats'], 'wo_31 and the amount asked' );
t_eq( array( $tx1, $tx2 ), $d['notice']['txids'], 'wo_31 and every transaction' );

// WO-31: overpaid completes too.
$d = NoticeDecision::decide( $make( 1043, $second[1], 'overpaid', $quote['sats'] + 500 ), $code->public, $lookup );
t_eq( 'complete', $d['action'], 'wo_31 an overpaid notice completes the order' );

// WO-32: a notice for an order already paid changes nothing.
$orders[1042]['paid'] = true;
$d                    = NoticeDecision::decide( $frame, $code->public, $lookup );
t_eq( array( 'action' => 'none', 'reason' => 'already_paid' ), $d, 'wo_32 a notice for a paid order changes nothing' );
$d = NoticeDecision::decide( $make( 1042, $taken[1], 'part_paid', 100 ), $code->public, $lookup );
t_eq( 'none', $d['action'], 'wo_32 any notice for a paid order changes nothing' );

// WO-31: part paid and paid late are notes, never a completion.
foreach ( array( 'part_paid', 'paid_late' ) as $state ) {
	$d = NoticeDecision::decide( $make( 1043, $second[1], $state, 1000 ), $code->public, $lookup );
	t_eq( 'note', $d['action'], "wo_31 $state is a note" );
	t_ok( ! isset( $d['transaction_id'] ), "wo_31 $state has no transaction to complete with" );
}

// WO-32: the same note is not made twice.
$partial = $make( 1043, $second[1], 'part_paid', 1000 );
$d       = NoticeDecision::decide( $partial, $code->public, $lookup );
$orders[1043]['last_notice'] = $d['key'];
t_eq( array( 'action' => 'none', 'reason' => 'repeat' ), NoticeDecision::decide( $partial, $code->public, $lookup ), 'wo_32 a repeated notice changes nothing' );
$d = NoticeDecision::decide( $make( 1043, $second[1], 'part_paid', 1500 ), $code->public, $lookup );
t_eq( 'note', $d['action'], 'wo_32 a notice with another amount is noted' );
$d = NoticeDecision::decide( $make( 1043, $second[1], 'paid', $quote['sats'] ), $code->public, $lookup );
t_eq( 'complete', $d['action'], 'wo_31 paid after part paid completes the order' );

// WO-32: a wrong signature, a wrong key and a wrong address are refused.
$orders[1042]['paid'] = false;
$flipped              = $frame;
$flipped[ strlen( $flipped ) - 1 ] = chr( ord( $flipped[ strlen( $flipped ) - 1 ] ) ^ 1 );
$d = NoticeDecision::decide( $flipped, $code->public, $lookup );
t_eq( array( 'action' => 'refuse', 'reason' => 'Signature' ), $d, 'wo_32 a wrong signature is refused' );
$store_key = sodium_crypto_sign_secretkey( sodium_crypto_sign_seed_keypair( str_repeat( "\x44", 32 ) ) );
$forged    = t_notice_frame(
	array(
		'order'        => 1042,
		'address'      => $taken[1],
		'state'        => 'paid',
		'counted_sats' => 1,
		'depth'        => 6,
		'txids'        => array( $tx1 ),
	),
	$store_key
);
$d = NoticeDecision::decide( $forged, $code->public, $lookup );
t_eq( 'refuse', $d['action'], 'wo_43 a notice signed with another key is refused' );
t_eq( 'Signature', $d['reason'], 'wo_43 for its signature' );
$d = NoticeDecision::decide( $make( 1042, $second[1], 'paid', $quote['sats'] ), $code->public, $lookup );
t_eq( array( 'action' => 'refuse', 'reason' => 'Address', 'order' => 1042 ), $d, 'wo_30 a notice with the address of another order is refused' );
$d = NoticeDecision::decide( $make( 9999, $second[1], 'paid', $quote['sats'] ), $code->public, $lookup );
t_eq( array( 'action' => 'refuse', 'reason' => 'UnknownOrder', 'order' => 9999 ), $d, 'wo_30 a notice for no order is refused' );
$d = NoticeDecision::decide( 'junk', $code->public, $lookup );
t_eq( 'refuse', $d['action'], 'wo_32 bytes that are not a notice are refused' );
$d = NoticeDecision::decide( $frame . "\0", $code->public, $lookup );
t_eq( 'refuse', $d['action'], 'wo_32 a notice with a byte more is refused' );

// A refusal never completes anything, whatever the frame.
foreach ( array( $flipped, $forged, 'junk', '' ) as $i => $bad ) {
	t_ok( 'complete' !== NoticeDecision::decide( $bad, $code->public, $lookup )['action'], "wo_32 refused frame $i completes nothing" );
}

// WO-11: an address is never given twice, over a whole run of orders.
$pool = new AddressPool( 'idididid' );
$pool->add( 0, array_map( static function ( int $i ): string { return 'addr' . $i; }, range( 0, 49 ) ), 'idididid' );
$given = array();
for ( $order = 1; $order <= 50; $order++ ) {
	$entry = $pool->take( $order );
	t_ok( ! in_array( $entry[1], $given, true ), "wo_11 order $order has an address of its own" );
	$given[] = $entry[1];
}
t_eq( null, $pool->take( 51 ), 'wo_11 with none left no address is given' );
t_done();
