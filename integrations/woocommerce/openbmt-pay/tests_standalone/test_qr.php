<?php
/**
 * WO-16, WO-50: the QR encoder against the matrices of the Rust encoder, the
 * link and code text of the payment page, and the SVG drawn from a matrix.
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\PaymentUri;
use OpenBmtPay\Qr;

$v = t_vectors( 'qr.json' );

/**
 * The rows of a vector as a flat list of modules.
 *
 * @param array<string,mixed> $case A case of the vectors.
 * @return bool[]
 */
function t_modules( array $case ): array {
	$out = array();
	foreach ( $case['rows'] as $row ) {
		$bytes = hex2bin( $row );
		for ( $x = 0; $x < $case['size']; $x++ ) {
			$out[] = ( ord( $bytes[ $x >> 3 ] ) & ( 0x80 >> ( $x & 7 ) ) ) !== 0;
		}
	}
	return $out;
}

// WO-16: the link and the code text are those of the Rust side.
foreach ( $v['links'] as $l ) {
	$label = hex2bin( $l['label'] );
	t_eq( $l['link'], PaymentUri::link( $l['address'], $l['sats'], $label ), 'wo_16 the link of ' . $l['address'] . ' ' . $l['sats'] );
	t_eq( $l['qr_text'], PaymentUri::qr_text( $l['address'], $l['sats'], $label ), 'wo_16 the code text of ' . $l['address'] . ' ' . $l['sats'] );
}

// The matrices, exactly.
$levels = array();
$modes  = array();
foreach ( $v['codes'] as $case ) {
	$text = hex2bin( $case['text'] );
	$code = Qr::encode( $text, $case['ecc'] );
	$name = 'qr ' . $case['ecc'] . ' ' . $case['mode'] . ' ' . strlen( $text ) . ' bytes';
	t_eq( $case['version'], $code['version'], "$name version" );
	t_eq( $case['mask'], $code['mask'], "$name mask" );
	t_eq( $case['mode'], $code['mode'], "$name mode" );
	t_eq( $case['size'], $code['size'], "$name size" );
	t_ok( t_modules( $case ) === $code['modules'], "$name matrix" );
	$levels[ $case['ecc'] ] = true;
	$modes[ $case['mode'] ] = true;
}
t_eq( array( 'H', 'L', 'M', 'Q' ), (function () use ( $levels ) { $k = array_keys( $levels ); sort( $k ); return $k; })(), 'qr every level is in the vectors' );
t_eq( 3, count( $modes ), 'qr every mode is in the vectors' );
t_ok( max( array_column( $v['codes'], 'version' ) ) >= 20, 'qr large versions are in the vectors' );
t_ok( min( array_column( $v['codes'], 'version' ) ) === 1, 'qr version 1 is in the vectors' );

// The plugin's longest link fits at the level the plugin uses.
$longest = '';
foreach ( $v['links'] as $l ) {
	if ( strlen( $l['qr_text'] ) > strlen( $longest ) ) {
		$longest = $l['qr_text'];
	}
}
t_ok( strlen( $longest ) > 280, 'qr the longest link is long' );
t_ok( Qr::encode( $longest, 'M' )['version'] <= 20, 'qr the longest link fits level M' );

// Refusals.
t_refused( static function () {
	Qr::encode( 'x', 'Z' );
}, 'Level', 'qr a level that is not one of the four' );
t_refused( static function () {
	Qr::encode( str_repeat( 'a', 3000 ), 'H' );
}, 'TooLong', 'qr text no code holds' );
t_eq( 21, Qr::encode( '', 'L' )['size'], 'qr empty text is version 1' );
t_eq( 'byte', Qr::encode( "a\n", 'L' )['mode'], 'qr a line feed is byte mode' );
t_eq( 'numeric', Qr::encode( '12', 'L' )['mode'], 'qr digits are numeric mode' );

// The SVG holds the matrix and nothing else.
foreach ( array( 'M', 'H' ) as $ecc ) {
	$code = Qr::encode( $longest, $ecc );
	$svg  = Qr::svg( $code );
	$size = $code['size'];
	$all  = $size + 8;
	$form = '/^<svg xmlns="http:\/\/www\.w3\.org\/2000\/svg" viewBox="0 0 ' . $all . ' ' . $all . '" shape-rendering="crispEdges"><rect width="' . $all . '" height="' . $all . '" fill="#fff"\/><path fill="#000" d="([Mhvz0-9 \-]*)"\/><\/svg>$/';
	t_ok( 1 === preg_match( $form, $svg, $m ), "svg $ecc has a fixed form" );
	// Draw the path back into a matrix.
	$back = array_fill( 0, $size * $size, false );
	t_ok( preg_match_all( '/M(\d+) (\d+)h(\d+)v1h-(\d+)z/', $m[1], $runs, PREG_SET_ORDER ) > 0, "svg $ecc has runs" );
	$consumed = 0;
	foreach ( $runs as $r ) {
		t_eq( $r[3], $r[4], "svg $ecc run is a rectangle" );
		for ( $i = 0; $i < (int) $r[3]; $i++ ) {
			$back[ ( (int) $r[2] - 4 ) * $size + (int) $r[1] - 4 + $i ] = true;
		}
		$consumed += strlen( $r[0] );
	}
	t_eq( strlen( $m[1] ), $consumed, "svg $ecc path is only runs" );
	t_ok( $back === $code['modules'], "svg $ecc path is the matrix" );
	t_ok( false === strpos( $svg, 'script' ) && false === strpos( $svg, 'href' ), "svg $ecc has no script and no link" );
	t_eq( 0, preg_match( '/[^\x20-\x7e]/', $svg ), "svg $ecc is plain ASCII" );
}
t_done();
