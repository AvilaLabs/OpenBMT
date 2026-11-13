<?php
/**
 * WO-10, WO-11: damaged stored data hides Bitcoin and is never read as an
 * empty pool; a range is bound to its pairing, so one of an earlier pairing
 * is refused after pairing again, and a fresh pool takes the first range of
 * its own pairing only.
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\AddressPool;
use OpenBmtPay\AddressRange;
use OpenBmtPay\Bytes;
use OpenBmtPay\Health;
use OpenBmtPay\NonceLedger;
use OpenBmtPay\Pairing;

$v      = t_vectors( 'web_order.json' );
$pair   = sodium_crypto_sign_seed_keypair( Bytes::from_hex( $v['key']['seed'] ) );
$public = sodium_crypto_sign_publickey( $pair );
$secret = sodium_crypto_sign_secretkey( $pair );
$list   = $v['range']['addresses'];

/**
 * A signed range for a pairing id, made as the Rust side makes one.
 *
 * @param string[] $addresses The addresses.
 */
function t_range_frame( int $start, array $addresses, string $id, string $key ): string {
	$body = AddressRange::MAGIC . chr( 1 ) . $id . pack( 'N', $start ) . chr( count( $addresses ) );
	foreach ( $addresses as $a ) {
		$body .= chr( strlen( $a ) ) . $a;
	}
	return $body . sodium_crypto_sign_detached( $body, $key );
}

$code_a = Pairing::from_text( $v['pairing']['text'] );
$id_a   = $code_a->id();
$id_b   = Pairing::id_of( str_repeat( "\x5a", 32 ) );
t_ok( $id_a !== $id_b, 'wo_10 two secrets have two ids' );

// The frame the PHP side makes is the frame the Rust side made.
t_eq( Bytes::from_hex( $v['range']['frame'] ), t_range_frame( $v['range']['start'], $list, $id_a, $secret ), 'wo_10 the range made here is the one made there' );

// WO-11: the state of the kept data.
$pool_a = new AddressPool( $id_a );
$pool_a->add( 7, $list, $id_a );
$stored = json_decode( json_encode( $pool_a->to_array() ), true );
$ledger = json_decode( json_encode( ( new NonceLedger() )->to_array() ), true );

$h = Health::inspect( true, $stored, $ledger );
t_eq( Health::READY, $h['state'], 'wo_11 sound data is ready' );
t_ok( Health::shows_bitcoin( $h ), 'wo_11 sound data with addresses shows Bitcoin' );
t_eq( 5, $h['pool']->remaining(), 'wo_11 and the pool is the stored one' );

$h = Health::inspect( false, $stored, $ledger );
t_eq( Health::UNPAIRED, $h['state'], 'wo_07 no pairing is unpaired' );
t_ok( ! Health::shows_bitcoin( $h ), 'wo_07 unpaired hides Bitcoin' );

$empty = json_decode( json_encode( ( new AddressPool( $id_a ) )->to_array() ), true );
$h     = Health::inspect( true, $empty, $ledger );
t_eq( Health::READY, $h['state'], 'wo_11 an empty pool is not damaged' );
t_ok( ! Health::shows_bitcoin( $h ), 'wo_11 an empty pool hides Bitcoin' );

foreach (
	array(
		'pool missing'      => array( null, $ledger ),
		'pool not an array' => array( 'x', $ledger ),
		'pool emptied'      => array( array(), $ledger ),
		'pool damaged'      => array( array_merge( $stored, array( 'queue' => 'x' ) ), $ledger ),
		'ledger missing'    => array( $stored, null ),
		'ledger damaged'    => array( $stored, array( 'zz' => 1 ) ),
		'both missing'      => array( null, null ),
	) as $name => $data
) {
	$h = Health::inspect( true, $data[0], $data[1] );
	t_eq( Health::DAMAGED, $h['state'], "wo_11 $name is damaged" );
	t_ok( ! Health::shows_bitcoin( $h ), "wo_11 $name hides Bitcoin" );
	t_ok( null === $h['pool'] || null === $h['nonces'], "wo_11 $name gives no empty pool to use" );
}
// Damaged data hides Bitcoin even though addresses are stored in the pool.
$h = Health::inspect( true, $stored, 'x' );
t_ok( ! Health::shows_bitcoin( $h ), 'wo_11 a good pool beside a damaged ledger still hides Bitcoin' );

// WO-10: a range is bound to its pairing.
$frame_a = t_range_frame( 7, $list, $id_a, $secret );
$frame_b = t_range_frame( 0, $list, $id_b, $secret );
$open    = AddressRange::open( $frame_a, $public, $id_a );
t_eq( $list, $open['addresses'], 'wo_10 a range of the pairing opens' );
t_refused( static function () use ( $frame_a, $public, $id_b ) {
	AddressRange::open( $frame_a, $public, $id_b );
}, 'Pairing', 'wo_10 a range of an earlier pairing is refused after pairing again' );
t_refused( static function () use ( $frame_b, $public, $id_a ) {
	AddressRange::open( $frame_b, $public, $id_a );
}, 'Pairing', 'wo_10 a range of a later pairing is refused by the earlier one' );
t_refused( static function () use ( $frame_a, $public ) {
	AddressRange::open( $frame_a, $public, '' );
}, 'Pairing', 'wo_10 a store with no pairing id takes no range' );
$flipped        = $frame_a;
$flipped[5]     = chr( ord( $flipped[5] ) ^ 1 );
t_refused( static function () use ( $flipped, $public, $id_a ) {
	AddressRange::open( $flipped, $public, $id_a );
}, 'Signature', 'wo_10 a changed id is a changed signature' );

// Pairing again: a fresh pool for the new pairing takes the first range of that pairing only.
$pool_b = new AddressPool( $id_b );
t_eq( AddressPool::WRONG_PAIRING, $pool_b->add( 7, $list, $id_a ), 'wo_10 the fresh pool refuses the range of the earlier pairing' );
t_eq( 0, $pool_b->remaining(), 'wo_10 and holds nothing' );
t_eq( null, $pool_b->next_index(), 'wo_10 and has no index' );
$open_b = AddressRange::open( $frame_b, $public, $id_b );
t_eq( AddressPool::ACCEPTED, $pool_b->add( $open_b['start'], $open_b['addresses'], $id_b ), 'wo_10 the first range of the new pairing is accepted' );
t_eq( 5, $pool_b->remaining(), 'wo_10 and the new pool holds it' );
t_eq( AddressPool::WRONG_PAIRING, $pool_b->add( 5, $list, $id_a ), 'wo_10 later ranges of the earlier pairing are still refused' );
$stored_b = AddressPool::from_array( json_decode( json_encode( $pool_b->to_array() ), true ) );
t_eq( AddressPool::WRONG_PAIRING, $stored_b->add( 5, $list, $id_a ), 'wo_10 the refusal survives storage' );

// The id inside the pool cannot be switched by changing the stored data to another valid one without the range ids matching.
$swapped               = $pool_b->to_array();
$swapped['pairing_id'] = bin2hex( $id_a );
$loaded                = AddressPool::from_array( $swapped );
t_eq( AddressPool::WRONG_PAIRING, $loaded->add( 5, $list, $id_b ), 'wo_10 the pool answers to the id it holds' );
t_done();
