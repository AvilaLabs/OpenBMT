<?php
/**
 * WO-20: the HMAC check of a request, its time window of five minutes and
 * the number used once.
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\NonceLedger;
use OpenBmtPay\RequestChecker;

$secret = str_repeat( "\x11", 32 );
$route  = '/openbmt/v1/orders';
$body   = '{"since":41}';
$now    = 1800000000;

/**
 * A request as the owner phone makes it.
 *
 * @return array<string,string>
 */
function t_request( string $secret, string $route, string $body, int $time, int $nonce ): array {
	$nonce_hex = bin2hex( str_repeat( chr( $nonce ), 16 ) );
	return array(
		'time'  => (string) $time,
		'nonce' => $nonce_hex,
		'mac'   => RequestChecker::mac( $secret, 'POST', $route, (string) $time, $nonce_hex, $body ),
	);
}

$check = static function ( array $r, int $now, NonceLedger $ledger ) use ( $secret, $route, $body ): void {
	RequestChecker::check( $secret, 'POST', $route, $r['time'], $r['nonce'], $r['mac'], $body, $now, $ledger );
};

t_eq( 300, RequestChecker::WINDOW_SECS, 'wo_20 the window is five minutes' );

// A good request is accepted once.
$ledger = new NonceLedger();
$r      = t_request( $secret, $route, $body, $now, 1 );
$check( $r, $now, $ledger );
t_ok( true, 'wo_20 a good request is accepted' );
t_refused( static function () use ( $check, $r, $now, $ledger ) {
	$check( $r, $now, $ledger );
}, 'Replay', 'wo_20 the same request again is refused' );
t_refused( static function () use ( $check, $now, $ledger, $secret, $route, $body ) {
	$check( t_request( $secret, $route, $body, $now + 5, 1 ), $now, $ledger );
}, 'Replay', 'wo_20 the same number with a new time is refused' );
$check( t_request( $secret, $route, $body, $now, 2 ), $now, $ledger );
t_ok( true, 'wo_20 another number is accepted' );

// The window: 300 seconds either side.
$ledger = new NonceLedger();
$check( t_request( $secret, $route, $body, $now - 300, 3 ), $now, $ledger );
$check( t_request( $secret, $route, $body, $now + 300, 4 ), $now, $ledger );
t_ok( true, 'wo_20 the edges of the window are accepted' );
t_refused( static function () use ( $check, $now, $ledger, $secret, $route, $body ) {
	$check( t_request( $secret, $route, $body, $now - 301, 5 ), $now, $ledger );
}, 'Time', 'wo_20 a request 301 seconds old is refused' );
t_refused( static function () use ( $check, $now, $ledger, $secret, $route, $body ) {
	$check( t_request( $secret, $route, $body, $now + 301, 6 ), $now, $ledger );
}, 'Time', 'wo_20 a request 301 seconds ahead is refused' );
t_refused( static function () use ( $check, $now, $ledger, $secret, $route, $body ) {
	$check( t_request( $secret, $route, $body, $now - 301, 5 ), $now, $ledger );
}, 'Time', 'wo_20 a refused time does not use the number' );

// The MAC is checked before the time and the number, and a refused request
// uses no number.
$ledger = new NonceLedger();
$r      = t_request( $secret, $route, $body, $now - 1000, 7 );
$r['mac'][0] = 'a' === $r['mac'][0] ? 'b' : 'a';
t_refused( static function () use ( $check, $r, $now, $ledger ) {
	$check( $r, $now, $ledger );
}, 'Mac', 'wo_20 a wrong MAC is refused before the time' );
t_eq( array(), $ledger->to_array(), 'wo_20 a wrong MAC keeps no number' );

// Each part of the request is in the MAC.
$good = t_request( $secret, $route, $body, $now, 0xab );
$bad  = array(
	'secret' => static function () use ( $good, $now, $route, $body ) {
		RequestChecker::check( str_repeat( "\x12", 32 ), 'POST', $route, $good['time'], $good['nonce'], $good['mac'], $body, $now, new NonceLedger() );
	},
	'method' => static function () use ( $good, $now, $route, $body, $secret ) {
		RequestChecker::check( $secret, 'GET', $route, $good['time'], $good['nonce'], $good['mac'], $body, $now, new NonceLedger() );
	},
	'route'  => static function () use ( $good, $now, $body, $secret ) {
		RequestChecker::check( $secret, 'POST', '/openbmt/v1/notice', $good['time'], $good['nonce'], $good['mac'], $body, $now, new NonceLedger() );
	},
	'time'   => static function () use ( $good, $now, $route, $body, $secret ) {
		RequestChecker::check( $secret, 'POST', $route, (string) ( (int) $good['time'] + 1 ), $good['nonce'], $good['mac'], $body, $now, new NonceLedger() );
	},
	'nonce'  => static function () use ( $good, $now, $route, $body, $secret ) {
		RequestChecker::check( $secret, 'POST', $route, $good['time'], bin2hex( str_repeat( "\x09", 16 ) ), $good['mac'], $body, $now, new NonceLedger() );
	},
	'body'   => static function () use ( $good, $now, $route, $secret ) {
		RequestChecker::check( $secret, 'POST', $route, $good['time'], $good['nonce'], $good['mac'], '{"since":0}', $now, new NonceLedger() );
	},
);
foreach ( $bad as $name => $call ) {
	t_refused( $call, 'Mac', "wo_20 a changed $name is refused" );
}

// Headers that are not what they should be are refused without a MAC check.
foreach (
	array(
		'time empty'   => array( '', $good['nonce'], $good['mac'] ),
		'time sign'    => array( '-1', $good['nonce'], $good['mac'] ),
		'time long'    => array( '1234567890123456', $good['nonce'], $good['mac'] ),
		'nonce short'  => array( $good['time'], 'abcd', $good['mac'] ),
		'nonce upper'  => array( $good['time'], strtoupper( $good['nonce'] ), $good['mac'] ),
		'mac short'    => array( $good['time'], $good['nonce'], 'abcd' ),
		'mac upper'    => array( $good['time'], $good['nonce'], strtoupper( $good['mac'] ) ),
	) as $name => $h
) {
	t_refused( static function () use ( $h, $secret, $route, $body, $now ) {
		RequestChecker::check( $secret, 'POST', $route, $h[0], $h[1], $h[2], $body, $now, new NonceLedger() );
	}, 'Headers', "wo_20 headers: $name" );
}
t_refused( static function () use ( $good, $route, $body, $now ) {
	RequestChecker::check( 'short', 'POST', $route, $good['time'], $good['nonce'], $good['mac'], $body, $now, new NonceLedger() );
}, 'Headers', 'wo_20 a secret that is not 32 bytes is refused' );
t_refused( static function () use ( $good, $secret, $body, $now ) {
	RequestChecker::check( $secret, 'post', '/a', $good['time'], $good['nonce'], $good['mac'], $body, $now, new NonceLedger() );
}, 'Headers', 'wo_20 a lower case method is refused' );
t_refused( static function () use ( $good, $secret, $body, $now ) {
	RequestChecker::check( $secret, 'POST', "/a\n/b", $good['time'], $good['nonce'], $good['mac'], $body, $now, new NonceLedger() );
}, 'Headers', 'wo_20 a route with a line feed is refused' );

// The ledger forgets numbers whose time is out of the window, and is bounded.
$ledger = new NonceLedger();
$ledger->use_once( str_repeat( 'a', 32 ), $now, $now );
$ledger->use_once( str_repeat( 'b', 32 ), $now + 100, $now + 100 );
$ledger->use_once( str_repeat( 'c', 32 ), $now + 500, $now + 500 );
t_ok( isset( $ledger->to_array()[ str_repeat( 'c', 32 ) ] ), 'wo_20 the new number is kept' );
t_eq( array( str_repeat( 'c', 32 ) ), array_keys( $ledger->to_array() ), 'wo_20 old numbers are forgotten' );
$ledger = new NonceLedger();
for ( $i = 0; $i < NonceLedger::MAX; $i++ ) {
	$ledger->use_once( str_pad( dechex( $i ), 32, '0', STR_PAD_LEFT ), $now, $now );
}
t_refused( static function () use ( $ledger, $now ) {
	$ledger->use_once( str_repeat( 'f', 32 ), $now, $now );
}, 'Full', 'wo_20 a full ledger refuses' );
$copy = NonceLedger::from_array( json_decode( json_encode( $ledger->to_array() ), true ) );
t_eq( NonceLedger::MAX, count( $copy->to_array() ), 'wo_20 the ledger survives storage' );
t_eq( array(), NonceLedger::from_array( array() )->to_array(), 'wo_20 an empty ledger is not damaged' );
foreach ( array( 'x', null, array( 'zz' => 1 ), array( str_repeat( 'a', 32 ) => 'x' ), array( str_repeat( 'A', 32 ) => 1 ) ) as $i => $bad ) {
	t_eq( null, NonceLedger::from_array( $bad ), "wo_11 damaged ledger data $i is not a ledger" );
}
t_eq( null, NonceLedger::from_array( array_fill_keys( array_map( static function ( int $i ): string { return str_pad( dechex( $i ), 32, '0', STR_PAD_LEFT ); }, range( 0, NonceLedger::MAX ) ), 1 ) ), 'wo_11 a ledger over the cap is damaged' );

// Old numbers are forgotten before the cap is looked at: a busy store is never refused for them.
$ledger = new NonceLedger();
$base   = 1800000000;
for ( $i = 0; $i < 3000; $i++ ) {
	$time = $base + $i;
	$r    = t_request( $secret, $route, $body, $time, $i % 200 );
	// Each request has its own number: the nonce is made of the counter.
	$nonce_hex = str_pad( dechex( $i ), 32, '0', STR_PAD_LEFT );
	$mac       = RequestChecker::mac( $secret, 'POST', $route, (string) $time, $nonce_hex, $body );
	RequestChecker::check( $secret, 'POST', $route, (string) $time, $nonce_hex, $mac, $body, $time, $ledger );
}
t_ok( true, 'wo_20 3000 requests over 3000 seconds all pass' );
t_ok( count( $ledger->to_array() ) <= RequestChecker::WINDOW_SECS + 1, 'wo_20 the ledger holds only the window' );
// A burst that fills the ledger within the window is refused at the cap, and is accepted again once the window has passed.
$ledger = new NonceLedger();
for ( $i = 0; $i < NonceLedger::MAX; $i++ ) {
	$ledger->use_once( str_pad( dechex( $i ), 32, '0', STR_PAD_LEFT ), $base, $base );
}
t_refused( static function () use ( $ledger, $base ) {
	$ledger->use_once( str_repeat( 'b', 32 ), $base + 1, $base + 1 );
}, 'Full', 'wo_20 a burst inside the window fills the ledger' );
$ledger->use_once( str_repeat( 'b', 32 ), $base + 301, $base + 301 );
t_eq( 1, count( $ledger->to_array() ), 'wo_20 after the window every old number is forgotten and the request passes' );
t_done();
