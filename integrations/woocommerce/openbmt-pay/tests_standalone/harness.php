<?php
/**
 * A tiny test harness: checks are counted, a failure is printed, and the
 * script ends with a pass count and a status. PHP alone runs it.
 */

declare(strict_types=1);

require_once dirname( __DIR__ ) . '/src/load.php';

$GLOBALS['t_pass'] = 0;
$GLOBALS['t_fail'] = 0;

function t_ok( bool $condition, string $name ): void {
	if ( $condition ) {
		++$GLOBALS['t_pass'];
		return;
	}
	++$GLOBALS['t_fail'];
	fwrite( STDERR, "FAIL  $name\n" );
}

/**
 * @param mixed $want What is expected.
 * @param mixed $got  What came.
 */
function t_eq( $want, $got, string $name ): void {
	t_ok( $want === $got, $name . ' (want ' . json_encode( $want ) . ', got ' . json_encode( $got ) . ')' );
}

function t_refused( callable $call, string $reason, string $name ): void {
	try {
		$call();
	} catch ( OpenBmtPay\Refusal $e ) {
		t_eq( $reason, $e->reason, $name );
		return;
	}
	t_ok( false, $name . ' (not refused)' );
}

function t_any_refusal( callable $call, string $name ): void {
	try {
		$call();
	} catch ( OpenBmtPay\Refusal $e ) {
		t_ok( true, $name );
		return;
	}
	t_ok( false, $name . ' (not refused)' );
}

/**
 * @return array<string,mixed>
 */
function t_vectors( string $file ): array {
	$text = file_get_contents( __DIR__ . '/vectors/' . $file );
	if ( false === $text ) {
		fwrite( STDERR, "FAIL  cannot read vectors/$file\n" );
		exit( 1 );
	}
	$data = json_decode( $text, true, 32, JSON_THROW_ON_ERROR );
	return $data;
}

function t_done(): void {
	echo 'PASS ' . $GLOBALS['t_pass'] . ' ' . basename( $_SERVER['argv'][0] ) . "\n";
	exit( $GLOBALS['t_fail'] > 0 ? 1 : 0 );
}
