<?php
/**
 * RT-10 to RT-17: the reply reader against what the Rust reader makes of the
 * same bytes: every reply of the vectors for every source of the table, and
 * every cut of every real reply. A reply is refused as Rust refuses it or
 * read to the same price; nothing else may happen.
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\Rates;
use OpenBmtPay\Refusal;
use OpenBmtPay\ReplyReader;

$v = t_vectors( 'rates.json' );

const T_NAMES = array(
	'L' => 'TooLong',
	'D' => 'TooDeep',
	'U' => 'NotUtf8',
	'M' => 'Malformed',
	'T' => 'TrailingBytes',
	'K' => 'DuplicateKey',
	'F' => 'MissingField',
	'S' => 'WrongShape',
	'C' => 'WrongCurrency',
	'E' => 'SourceError',
	'a' => 'Price:Empty',
	'b' => 'Price:Sign',
	'c' => 'Price:Exponent',
	'd' => 'Price:BadCharacter',
	'e' => 'Price:TwoDots',
	'f' => 'Price:BadShape',
	'g' => 'Price:LeadingZero',
	'h' => 'Price:TooManyDecimals',
	'i' => 'Price:Zero',
	'j' => 'Price:OutOfBand',
);

/**
 * What the reader makes of a reply, as the letter of the vectors.
 *
 * @param array<int,mixed> $source The row of the table.
 */
function t_code( array $source, string $body, ?int &$minor ): string {
	$minor = null;
	try {
		$minor = ReplyReader::minor( $source, $body );
		return '+';
	} catch ( Refusal $e ) {
		$letter = array_search( $e->reason, T_NAMES, true );
		return false === $letter ? '?' . $e->reason : (string) $letter;
	} catch ( Throwable $e ) {
		return '!' . get_class( $e ) . ' ' . $e->getMessage();
	}
}

t_eq( 39, count( Rates::TABLE ), 'rt_15 the table has 39 sources' );
t_ok( count( $v['replies'] ) >= 150, 'rt_15 there are many replies' );
foreach ( $v['replies'] as $reply ) {
	$body = hex2bin( $reply['body'] ) . str_repeat( ' ', $reply['pad'] );
	foreach ( Rates::TABLE as $i => $source ) {
		$got = t_code( $source, $body, $minor );
		t_eq( $reply['codes'][ $i ], $got, 'rt_15 ' . $reply['name'] . ' read as ' . $source[0] );
		if ( '+' === $reply['codes'][ $i ] ) {
			t_eq( $reply['minors'][ $source[0] ], $minor, 'rt_15 ' . $reply['name'] . ' price of ' . $source[0] );
		}
	}
}

// Every cut of every real reply: refused as Rust refuses it, never a crash.
t_eq( 30, count( $v['truncations'] ), 'rt_13 every fixture is cut' );
foreach ( $v['truncations'] as $cut ) {
	$source = Rates::source( $cut['source'] );
	$body   = hex2bin( $cut['body'] );
	t_eq( strlen( $body ) + 1, strlen( $cut['codes'] ), 'rt_13 a code for every length of ' . $cut['source'] );
	$wrong = 0;
	for ( $n = 0, $len = strlen( $body ); $n <= $len; $n++ ) {
		$got = t_code( $source, substr( $body, 0, $n ), $minor );
		if ( $got !== $cut['codes'][ $n ] ) {
			++$wrong;
			t_eq( $cut['codes'][ $n ], $got, 'rt_13 ' . $cut['source'] . ' cut at ' . $n );
		}
	}
	t_eq( 0, $wrong, 'rt_13 every cut of ' . $cut['source'] . ' is read as Rust reads it' );
	// A cut shorter than the whole reply is never a price, apart from white space at its end.
	t_ok( '+' === $cut['codes'][ strlen( $body ) ], 'rt_13 the whole of ' . $cut['source'] . ' is a price' );
	t_ok( 'M' === $cut['codes'][0], 'rt_13 nothing is malformed for ' . $cut['source'] );
}

// The reply reader takes the text of the price, and the quote takes it from there.
$kraken = Rates::source( 'kraken' );
t_eq( '73554.50000', ReplyReader::price_text( $kraken, hex2bin( bin2hex( '{"error":[],"result":{"X":{"c":["73554.50000","1"]}}}' ) ) ), 'rt_15 the text of the price' );
t_done();
