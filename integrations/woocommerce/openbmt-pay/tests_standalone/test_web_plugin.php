<?php
/**
 * The WordPress layer, run against the stubs of tests_e2e: the store's lock
 * (WO-11), a second payment attempt of an order, who may pair (WO-05) and
 * the request check that does not take the lock (WO-20).
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\AddressPool;
use OpenBmtPay\Health;
use OpenBmtPay\NonceLedger;
use OpenBmtPay\Pairing;
use OpenBmtPay\RequestChecker;

$state = sys_get_temp_dir() . '/obm_plugin_test_' . bin2hex( random_bytes( 4 ) ) . '.json';
putenv( 'OBM_E2E_STATE=' . $state );
require_once dirname( __DIR__ ) . '/tests_e2e/load_plugin.php';
// Registered after the stubs' own save, so it runs last.
register_shutdown_function(
	static function () use ( $state ) {
		if ( is_file( $state ) ) {
			unlink( $state );
		}
	}
);

/** What the admin page's redirect and a refused user stop with. */
final class T_Stop extends Exception {
}

function t_reset(): void {
	$GLOBALS['obm_e2e_options'] = array();
	$GLOBALS['obm_e2e_orders']  = array();
	$GLOBALS['wpdb']->inserts   = 0;
	$GLOBALS['wpdb']->before    = null;
}

$id     = "\xab\xcd\xef\x01\x23\x45\x67\x89";
$secret = str_repeat( "\x11", 32 );

/** A paired store with a pool of five addresses. */
function t_pair_store( string $id, string $secret ): void {
	$pool = new AddressPool( $id );
	$pool->add( 0, array( 'a0', 'a1', 'a2', 'a3', 'a4' ), $id );
	$GLOBALS['obm_e2e_options'][ OpenBmt_Pay_Store::OPT_PAIRING ] = array(
		'public'    => bin2hex( str_repeat( "\x22", 32 ) ),
		'secret'    => bin2hex( $secret ),
		'name'      => 'Shop',
		'paired_at' => 1,
	);
	$GLOBALS['obm_e2e_options'][ OpenBmt_Pay_Store::OPT_POOL ]   = $pool->to_array();
	$GLOBALS['obm_e2e_options'][ OpenBmt_Pay_Store::OPT_NONCES ] = ( new NonceLedger() )->to_array();
}

$wpdb = $GLOBALS['wpdb'];
$lock = OpenBmt_Pay_Store::OPT_LOCK;
$opts = &$GLOBALS['obm_e2e_options'];

// WO-11: the lock is one INSERT IGNORE that only one request can win.
t_reset();
$ran = OpenBmt_Pay_Store::with_lock(
	static function () use ( $lock ) {
		return array_key_exists( $lock, $GLOBALS['obm_e2e_options'] );
	}
);
t_eq( true, $ran, 'wo_11 the step runs with the lock held' );
t_ok( ! array_key_exists( $lock, $opts ), 'wo_11 the lock is released after the step' );
t_eq( 1, $wpdb->inserts, 'wo_11 one try took it' );

$first = OpenBmt_Pay_Store::lock_try();
t_ok( null !== $first && 1 === preg_match( '/^[0-9a-f]{16}:[0-9]+$/', $first ), 'wo_11 the value is a random token and the time' );
t_eq( null, OpenBmt_Pay_Store::lock_try(), 'wo_11 a second request does not get a held lock' );
t_eq( $first, $opts[ $lock ], 'wo_11 the held lock is untouched by the second try' );
OpenBmt_Pay_Store::lock_release( $first );
t_ok( ! array_key_exists( $lock, $opts ), 'wo_11 released by its own value' );

// The race over a free lock: the other request's insert lands between this request's decision to try and its INSERT.
t_reset();
$other = null;
$wpdb->before = static function ( string $sql ) use ( $wpdb, &$other ) {
	if ( 0 === strpos( $sql, 'INSERT IGNORE' ) && null === $other ) {
		$hook         = $wpdb->before;
		$wpdb->before = null;
		$other        = OpenBmt_Pay_Store::lock_try();
		$wpdb->before = $hook;
	}
};
$mine = OpenBmt_Pay_Store::lock_try();
t_ok( null !== $other, 'wo_11 race over a free lock: the request that got in first holds it' );
t_eq( null, $mine, 'wo_11 race over a free lock: the other request does not' );
t_eq( $other, $opts[ $lock ], 'wo_11 race over a free lock: the row is the first holder\'s' );

// The race over a lock that was never released: both read the old value, one UPDATE wins.
t_reset();
$old          = 'deadbeefdeadbeef:' . ( time() - 1000 );
$opts[ $lock ] = $old;
$other        = null;
$wpdb->before = static function ( string $sql ) use ( $wpdb, &$other ) {
	if ( 0 === strpos( $sql, 'UPDATE' ) && null === $other ) {
		$hook         = $wpdb->before;
		$wpdb->before = null;
		$other        = OpenBmt_Pay_Store::lock_try();
		$wpdb->before = $hook;
	}
};
$mine = OpenBmt_Pay_Store::lock_try();
t_ok( null !== $other, 'wo_11 race over a stale lock: the request whose UPDATE came first holds it' );
t_eq( null, $mine, 'wo_11 race over a stale lock: the other request does not' );
t_eq( $other, $opts[ $lock ], 'wo_11 race over a stale lock: the row is the first holder\'s' );
// The one that lost does not release the winner's lock; a stale holder that wakes does not either.
OpenBmt_Pay_Store::lock_release( $old );
t_eq( $other, $opts[ $lock ], 'wo_11 a request whose lock was taken over leaves the new holder\'s lock alone' );
OpenBmt_Pay_Store::lock_release( $other );
t_ok( ! array_key_exists( $lock, $opts ), 'wo_11 the holder releases its own' );

// A stale lock is taken over by with_lock; a fresh one is not.
t_reset();
$opts[ $lock ] = 'deadbeefdeadbeef:' . ( time() - OpenBmt_Pay_Store::LOCK_SECS - 5 );
t_eq( 'ran', OpenBmt_Pay_Store::with_lock( static function () {
	return 'ran';
} ), 'wo_11 a lock older than LOCK_SECS is taken over' );
t_ok( ! array_key_exists( $lock, $opts ), 'wo_11 and released after' );
$opts[ $lock ] = 'deadbeefdeadbeef:' . ( time() - 5 );
t_eq( null, OpenBmt_Pay_Store::lock_try(), 'wo_11 a lock younger than LOCK_SECS is not taken over' );

// WO-11, WO-15: an order that has its address takes no other (WC_Payment_Gateway is a stand-in).
class WC_Payment_Gateway {
	public function get_return_url( $order ) {
		return 'return:' . $order->get_id();
	}
}
require_once dirname( __DIR__ ) . '/wc/class-openbmt-pay-rates.php';
require_once dirname( __DIR__ ) . '/wc/class-openbmt-pay-gateway.php';
$gateway = ( new ReflectionClass( 'OpenBmt_Pay_Gateway' ) )->newInstanceWithoutConstructor();
t_reset();
t_pair_store( $id, $secret );
$meta = array(
	'_openbmt_seq'        => '7',
	'_openbmt_index'      => '0',
	'_openbmt_address'    => 'a0',
	'_openbmt_sats'       => '50000',
	'_openbmt_quoted_at'  => '1700000000',
	'_openbmt_expires_at' => '1700001800',
);
$GLOBALS['obm_e2e_orders']['7'] = array(
	'payment_method' => 'openbmt',
	'status'         => 'pending',
	'meta'           => $meta,
);
$pool_before = $opts[ OpenBmt_Pay_Store::OPT_POOL ];
$answer      = $gateway->process_payment( 7 );
t_eq( array( 'result' => 'success', 'redirect' => 'return:7' ), $answer, 'wo_11 an order with an address goes to its payment page' );
t_eq( $meta, $GLOBALS['obm_e2e_orders']['7']['meta'], 'wo_11 its address, index, sats and quote are unchanged' );
t_eq( $pool_before, $opts[ OpenBmt_Pay_Store::OPT_POOL ], 'wo_11 the pool is unchanged' );
t_eq( 0, $wpdb->inserts, 'wo_11 the lock was not even tried' );
t_eq( 'pending', $GLOBALS['obm_e2e_orders']['7']['status'], 'wo_11 the order is not touched' );

// WO-10, WO-11: an order with no address takes one, the fields written with the lock held.
// The stubs of the quote: two sources that agree, and the notices kept.
function get_transient( string $key ) {
	$now = time();
	return array( array( 'kraken', $now, '73550.1' ), array( 'coinbase', $now, '73552.175' ) );
}
function set_transient( string $key, $value, int $secs ) {
}
function wc_format_decimal( $value, $places = null ) {
	return (string) $value;
}
function wc_add_notice( string $text, string $type = 'success' ) {
	$GLOBALS['t_notices'][] = $text;
}
$fresh_order = static function (): void {
	$GLOBALS['obm_e2e_orders']['8'] = array(
		'payment_method' => 'openbmt',
		'status'         => 'pending',
		'currency'       => 'USD',
		'total'          => '25.00',
		'meta'           => array(),
	);
};
t_reset();
t_pair_store( $id, $secret );
$fresh_order();
$answer = $gateway->process_payment( 8 );
t_eq( array( 'result' => 'success', 'redirect' => 'return:8' ), $answer, 'wo_10 an order with none takes an address and goes to its payment page' );
t_eq( 'a0', $GLOBALS['obm_e2e_orders']['8']['meta']['_openbmt_address'], 'wo_10 the first address of the pool is its address' );
t_eq( '2500', $GLOBALS['obm_e2e_orders']['8']['meta']['_openbmt_total_minor'], 'wo_10 its total and quote are recorded' );
t_eq( 'on-hold', $GLOBALS['obm_e2e_orders']['8']['status'], 'wo_10 and it is on hold' );

// WO-11: another submit of the order takes its address between this one's check and its lock; this one takes none.
t_reset();
t_pair_store( $id, $secret );
$fresh_order();
$pool_start = $opts[ OpenBmt_Pay_Store::OPT_POOL ];
$other      = array(
	'_openbmt_seq'     => '8',
	'_openbmt_index'   => '0',
	'_openbmt_address' => 'a0',
	'_openbmt_sats'    => '12345',
);
$fired = false;
$wpdb->before = static function ( string $sql ) use ( &$fired, $other ) {
	if ( ! $fired && 0 === strpos( $sql, 'INSERT IGNORE' ) ) {
		$fired = true;
		$GLOBALS['obm_e2e_orders']['8']['meta'] = $other;
	}
};
$answer = $gateway->process_payment( 8 );
$wpdb->before = null;
t_ok( $fired, 'wo_11 the other submit came between the check and the lock' );
t_eq( array( 'result' => 'success', 'redirect' => 'return:8' ), $answer, 'wo_11 the waiting submit goes to the payment page' );
t_eq( $other, $GLOBALS['obm_e2e_orders']['8']['meta'], 'wo_11 the first submit\'s address and quote are not overwritten' );
t_eq( $pool_start, $opts[ OpenBmt_Pay_Store::OPT_POOL ], 'wo_11 the waiting submit took no second address' );
t_eq( 'pending', $GLOBALS['obm_e2e_orders']['8']['status'], 'wo_11 and left the order alone' );
t_ok( ! array_key_exists( $lock, $opts ), 'wo_11 the lock is free after' );

// WO-05: only an administrator pairs or unpairs, and a pairing in use is not replaced.
function current_user_can( string $cap ): bool {
	$GLOBALS['t_asked'][] = $cap;
	return $GLOBALS['t_can'] === $cap;
}
function wp_die( $message = '', $title = '', $args = array() ) {
	throw new T_Stop( 'die:' . ( $args['response'] ?? 0 ) );
}
function check_admin_referer( string $action ) {
	return true;
}
function sanitize_text_field( $text ) {
	return (string) $text;
}
function wp_unslash( $text ) {
	return $text;
}
function add_query_arg( array $args, string $url ) {
	return $url . '?' . http_build_query( $args );
}
function admin_url( string $path ) {
	return $path;
}
function wp_safe_redirect( string $url ) {
	throw new T_Stop( $url );
}
require_once dirname( __DIR__ ) . '/wc/class-openbmt-pay-admin.php';

$act = static function ( string $method, string $can ) {
	$GLOBALS['t_can']   = $can;
	$GLOBALS['t_asked'] = array();
	try {
		call_user_func( array( 'OpenBmt_Pay_Admin', $method ) );
	} catch ( T_Stop $e ) {
		return $e->getMessage();
	}
	return 'returned';
};
t_eq( 'manage_options', OpenBmt_Pay_Admin::CAPABILITY, 'wo_05 the capability is the administrator\'s' );
t_reset();
t_pair_store( $id, $secret );
$before = $opts;
t_eq( 'die:403', $act( 'pair', 'manage_woocommerce' ), 'wo_05 a shop manager cannot pair' );
t_eq( 'die:403', $act( 'unpair', 'manage_woocommerce' ), 'wo_05 a shop manager cannot unpair' );
t_eq( 'die:403', $act( 'page', 'manage_woocommerce' ), 'wo_05 a shop manager does not see the pairing page' );
t_eq( $before, $opts, 'wo_05 and nothing was changed' );
$_POST['openbmt_pay_code'] = 'not a code';
t_eq( 'admin.php?page=openbmt-pay&result=paired-already', $act( 'pair', 'manage_options' ), 'wo_05 an administrator cannot pair over a pairing' );
t_eq( $before, $opts, 'wo_05 the pairing, pool and numbers are as they were' );
$GLOBALS['obm_e2e_options'][ OpenBmt_Pay_Store::OPT_POOL ] = 'damaged';
t_eq( 'admin.php?page=openbmt-pay&result=bad', $act( 'pair', 'manage_options' ), 'wo_05 damaged data may be paired over (here the code is read, and refused)' );
t_eq( 'admin.php?page=openbmt-pay&result=unpaired', $act( 'unpair', 'manage_options' ), 'wo_05 an administrator unpairs' );
t_eq( null, OpenBmt_Pay_Store::pairing(), 'wo_05 and the store is unpaired' );
t_eq( 'admin.php?page=openbmt-pay&result=bad', $act( 'pair', 'manage_options' ), 'wo_05 an unpaired store reads the code' );

// WO-20: a request that fails the HMAC or the time never takes the lock.
t_reset();
t_pair_store( $id, $secret );
$route   = '/openbmt/v1/orders';
$body    = '{"since":0}';
$request = static function ( int $time, string $nonce, ?string $mac = null ) use ( $secret, $route, $body ): WP_REST_Request {
	return new WP_REST_Request(
		'POST',
		$route,
		array(
			'X-OBM-Time'  => (string) $time,
			'X-OBM-Nonce' => $nonce,
			'X-OBM-Mac'   => $mac ?? RequestChecker::mac( $secret, 'POST', $route, (string) $time, $nonce, $body ),
		),
		$body
	);
};
$nonce = bin2hex( str_repeat( "\x01", 16 ) );
$now   = time();
$wpdb->inserts = 0;
t_ok( OpenBmt_Pay_Rest::authenticate( $request( $now, $nonce, str_repeat( 'a', 64 ) ) ) instanceof WP_Error, 'wo_20 a bad MAC is denied' );
t_ok( OpenBmt_Pay_Rest::authenticate( $request( $now - 301, $nonce ) ) instanceof WP_Error, 'wo_20 a stale time is denied' );
t_ok( OpenBmt_Pay_Rest::authenticate( $request( $now + 301, $nonce ) ) instanceof WP_Error, 'wo_20 a time ahead is denied' );
t_ok( OpenBmt_Pay_Rest::authenticate( new WP_REST_Request( 'POST', $route, array(), $body ) ) instanceof WP_Error, 'wo_20 missing headers are denied' );
t_eq( 0, $wpdb->inserts, 'wo_20 none of them took the lock' );
t_eq( true, OpenBmt_Pay_Rest::authenticate( $request( $now, $nonce ) ), 'wo_20 a good request is allowed' );
t_eq( 1, $wpdb->inserts, 'wo_20 and takes the lock once' );
t_ok( OpenBmt_Pay_Rest::authenticate( $request( $now, $nonce ) ) instanceof WP_Error, 'wo_20 the same request again is denied as a replay' );
t_eq( 2, $wpdb->inserts, 'wo_20 a replay takes the lock, for the ledger' );
t_ok( ! array_key_exists( $lock, $opts ), 'wo_20 the lock is free after' );
t_eq( Health::READY, OpenBmt_Pay_Store::health()['state'], 'wo_20 the store is still sound' );

t_done();
