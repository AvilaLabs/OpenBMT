<?php
/**
 * WO-50: the vectors the Rust side made: the key, the pairing code, a signed
 * range, a signed notice and an HMAC request are all read and checked here,
 * and every tampered form is refused.
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\AddressRange;
use OpenBmtPay\Bytes;
use OpenBmtPay\NonceLedger;
use OpenBmtPay\Notice;
use OpenBmtPay\Pairing;
use OpenBmtPay\RequestChecker;

$v = t_vectors( 'web_order.json' );

// WO-43: the same seed gives the same public key, and the same signature.
$seed    = Bytes::from_hex( $v['key']['seed'] );
$pair    = sodium_crypto_sign_seed_keypair( $seed );
$public  = sodium_crypto_sign_publickey( $pair );
$private = sodium_crypto_sign_secretkey( $pair );
t_eq( $v['key']['public'], bin2hex( $public ), 'wo_43 the public key of the seed' );
t_eq( $v['key']['fingerprint'], Pairing::fingerprint_of( $public ), 'wo_05 the fingerprint' );

// WO-05: the pairing code, from text and from bytes.
$p = $v['pairing'];
foreach ( array( Pairing::from_text( $p['text'] ), Pairing::from_bytes( Bytes::from_hex( $p['bytes'] ) ), Pairing::from_text( " \n" . $p['text'] . "\r\n" ) ) as $i => $code ) {
	t_eq( $public, $code->public, "wo_05 the public key in the code $i" );
	t_eq( $p['secret'], bin2hex( $code->secret ), "wo_05 the secret in the code $i" );
	t_eq( $p['shop_name'], $code->shop_name, "wo_05 the shop name in the code $i" );
	t_eq( $v['key']['fingerprint'], $code->fingerprint(), "wo_05 the fingerprint of the code $i" );
}
$bytes = Bytes::from_hex( $p['bytes'] );
for ( $at = 0, $n = strlen( $bytes ); $at < $n; $at++ ) {
	$changed        = $bytes;
	$changed[ $at ] = chr( ord( $changed[ $at ] ) ^ 1 );
	t_any_refusal( static function () use ( $changed ) {
		Pairing::from_bytes( $changed );
	}, "wo_05 a changed byte $at of the code is refused" );
	t_any_refusal( static function () use ( $bytes, $at ) {
		Pairing::from_bytes( substr( $bytes, 0, $at ) );
	}, "wo_05 a code cut at $at is refused" );
}
t_any_refusal( static function () use ( $bytes ) {
	Pairing::from_bytes( $bytes . "\0" );
}, 'wo_05 a code with a byte more is refused' );
t_any_refusal( static function () use ( $p ) {
	Pairing::from_text( strtolower( $p['text'] ) );
}, 'wo_05 lower case text is refused' );
t_any_refusal( static function () use ( $p ) {
	Pairing::from_text( $p['text'] . 'A' );
}, 'wo_05 text with a character more is refused' );
t_any_refusal( static function () use ( $p ) {
	Pairing::from_text( substr( $p['text'], 0, -1 ) );
}, 'wo_05 text with a character less is refused' );

// WO-10: the signed range.
$r     = $v['range'];
$frame = Bytes::from_hex( $r['frame'] );
$open  = AddressRange::open( $frame, $public );
t_eq( $r['start'], $open['start'], 'wo_10 the start of the range' );
t_eq( $r['addresses'], $open['addresses'], 'wo_10 the addresses of the range' );
t_eq(
	substr( $frame, -64 ),
	sodium_crypto_sign_detached( substr( $frame, 0, -64 ), $private ),
	'wo_10 the signature made here is the one made there'
);
foreach ( $r['tampered'] as $t ) {
	$bad = Bytes::from_hex( $t['frame'] );
	t_any_refusal( static function () use ( $bad, $public ) {
		AddressRange::open( $bad, $public );
	}, 'wo_10 tampered range: ' . $t['name'] );
}
t_ok( count( $r['tampered'] ) >= 8, 'wo_10 there are tampered ranges' );
$other = sodium_crypto_sign_publickey( sodium_crypto_sign_seed_keypair( str_repeat( "\x07", 32 ) ) );
t_refused( static function () use ( $frame, $other ) {
	AddressRange::open( $frame, $other );
}, 'Signature', 'wo_10 a range under another key is refused' );
t_any_refusal( static function () use ( $frame ) {
	AddressRange::open( $frame, 'short' );
}, 'wo_10 a key of the wrong size is refused' );

// WO-30: the signed notice.
$n      = $v['notice'];
$frame  = Bytes::from_hex( $n['frame'] );
$notice = Notice::open( $frame, $public );
t_eq( $n['order'], $notice['order'], 'wo_30 the order' );
t_eq( $n['address'], $notice['address'], 'wo_30 the address' );
t_eq( $n['state'], $notice['state'], 'wo_30 the state' );
t_eq( $n['counted_sats'], $notice['counted_sats'], 'wo_30 the amount counted' );
t_eq( $n['depth'], $notice['depth'], 'wo_30 the depth' );
t_eq( $n['txids'], $notice['txids'], 'wo_30 the transaction IDs' );
t_eq(
	substr( $frame, -64 ),
	sodium_crypto_sign_detached( substr( $frame, 0, -64 ), $private ),
	'wo_30 the signature made here is the one made there'
);
foreach ( $n['tampered'] as $t ) {
	$bad = Bytes::from_hex( $t['frame'] );
	t_any_refusal( static function () use ( $bad, $public ) {
		Notice::open( $bad, $public );
	}, 'wo_30 tampered notice: ' . $t['name'] );
}
t_ok( count( $n['tampered'] ) >= 10, 'wo_30 there are tampered notices' );
for ( $at = 0, $len = strlen( $frame ); $at < $len; $at++ ) {
	$changed        = $frame;
	$changed[ $at ] = chr( ord( $changed[ $at ] ) ^ 1 );
	t_any_refusal( static function () use ( $changed, $public ) {
		Notice::open( $changed, $public );
	}, "wo_30 a changed byte $at of the notice is refused" );
}

// WO-20: the HMAC request.
$q = $v['request'];
t_eq(
	$q['mac'],
	RequestChecker::mac( Bytes::from_hex( $q['secret'] ), $q['method'], $q['route'], (string) $q['time'], $q['nonce'], Bytes::from_hex( $q['body'] ) ),
	'wo_20 the MAC made here is the one made there'
);
$checks = static function ( array $x, int $now ): void {
	RequestChecker::check( Bytes::from_hex( $x['secret'] ), $x['method'], $x['route'], (string) $x['time'], $x['nonce'], $x['mac'], Bytes::from_hex( $x['body'] ), $now, new NonceLedger() );
};
$checks( $q, $q['time'] );
t_ok( true, 'wo_20 the request of the vector is accepted' );
foreach ( $v['request_tampered'] as $t ) {
	t_refused( static function () use ( $checks, $t, $q ) {
		$checks( $t['request'], $q['time'] );
	}, 'Mac', 'wo_20 tampered request: ' . $t['name'] );
}
t_ok( count( $v['request_tampered'] ) >= 7, 'wo_20 there are tampered requests' );
t_done();
