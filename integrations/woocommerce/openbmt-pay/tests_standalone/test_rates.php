<?php
/**
 * WO-15: the quote maker against the vectors of the Rust rate library
 * (docs/RATE-RULES.md, RT-01, RT-20 to RT-38, and CR-08).
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\Money;
use OpenBmtPay\Price;
use OpenBmtPay\Quote;
use OpenBmtPay\Rates;
use OpenBmtPay\Refusal;

$v = t_vectors( 'rates.json' );

// RT-01: the table is the Rust table.
t_eq( count( $v['table'] ), count( Rates::TABLE ), 'rt_01 the table has the same number of sources' );
foreach ( $v['table'] as $i => $row ) {
	t_eq( array( $row['id'], $row['initial'], $row['currency'], $row['url'] ), array_slice( Rates::TABLE[ $i ], 0, 4 ), 'rt_01 the source ' . $row['id'] );
}
foreach ( array_keys( Rates::BAND_FACTOR ) as $code ) {
	t_ok( count( Rates::sources_for( $code ) ) >= 2, "rt_01 $code has two sources at least" );
}

// RT-20 to RT-24: the text of a price.
foreach ( $v['prices'] as $case ) {
	$name = 'rt_20 ' . $case['currency'] . ' ' . json_encode( $case['text'] );
	try {
		$got = array( 'minor' => Price::parse_minor( $case['text'], $case['currency'] ) );
	} catch ( Refusal $e ) {
		$got = array( 'refused' => $e->reason );
	}
	t_eq( $case['expect'], $got, $name );
}
t_ok( count( $v['prices'] ) > 60, 'rt_20 there are many price cases' );

// RT-30 to RT-38: several prices into one rate.
foreach ( $v['combine'] as $case ) {
	$name = 'rt_30 ' . $case['name'];
	try {
		$samples = array();
		foreach ( $case['samples'] as $s ) {
			$samples[] = Quote::sample( $s['id'], $s['fetched_at'], $s['price'] );
		}
		$got = Quote::combine( $samples, $case['now'] );
	} catch ( Refusal $e ) {
		$got = array_merge( array( 'refused' => $e->reason ), $e->detail );
	}
	t_eq( $case['expect'], $got, $name );
}
t_ok( count( $v['combine'] ) >= 20, 'rt_30 there are many combine cases' );

// CR-08: the satoshis an amount needs.
foreach ( $v['sats'] as $case ) {
	t_eq( $case['sats'], Quote::sats_for( $case['amount'], $case['rate'] ), 'cr_08 ' . $case['amount'] . ' at ' . $case['rate'] );
}

// WO-15: a quote of an order, from readings.
$now      = 1800000000;
$readings = array(
	array( 'kraken', $now - 30, '73550.1' ),
	array( 'coinbase', $now - 10, '73552.175' ),
);
$quote = Quote::make( 'USD', 2500, $readings, $now );
t_eq( 7355010, $quote['rate'], 'wo_15 the rate is the lower price' );
t_eq( 'med.kc', $quote['label'], 'wo_15 the label' );
t_eq( Quote::sats_for( 2500, 7355010 ), $quote['sats'], 'wo_15 the satoshis' );
t_eq( $now, $quote['quoted_at'], 'wo_15 the quote time' );
t_eq( $now + 1800, $quote['expires_at'], 'wo_15 the quote lasts 30 minutes' );
t_refused( static function () use ( $now ) {
	Quote::make( 'USD', 2500, array( array( 'kraken', $now, '73550.1' ) ), $now );
}, 'NotEnoughSources', 'wo_15 one source makes no quote' );
t_refused( static function () use ( $now ) {
	Quote::make( 'USD', 2500, array( array( 'kraken', $now, 'x' ), array( 'coinbase', $now, 'y' ) ), $now );
}, 'NotEnoughSources', 'wo_15 readings that are refused make no quote' );
t_refused( static function () use ( $now ) {
	Quote::make( 'USD', 2500, array( array( 'kraken-eur', $now, '73550.1' ), array( 'coinbase-eur', $now, '73552.1' ) ), $now );
}, 'MixedCurrencies', 'wo_15 sources of another currency are refused' );
t_refused( static function () use ( $now ) {
	Quote::make( 'USD', 2500, array( array( 'nobody', $now, '73550.1' ), array( 'coinbase', $now, '73552.1' ) ), $now );
}, 'MixedCurrencies', 'wo_15 a source that is not in the table is refused' );
t_refused( static function () use ( $now, $readings ) {
	Quote::make( 'USD', 0, $readings, $now );
}, 'Zero', 'wo_15 an order of nothing makes no quote' );
t_refused( static function () use ( $now ) {
	Quote::make( 'USD', 2500, array( array( 'kraken', $now - 601, '73550.1' ), array( 'coinbase', $now - 700, '73552.1' ) ), $now );
}, 'TooOld', 'wo_15 old prices make no quote' );
t_refused( static function () use ( $now ) {
	Quote::make( 'USD', 2500, array( array( 'kraken', $now, '73550.1' ), array( 'coinbase', $now, '90000' ) ), $now );
}, 'PricesDisagree', 'wo_15 prices that disagree make no quote' );

// The order total as text.
t_eq( 1234, Money::minor_from_decimal( '12.34', 'USD' ), 'money 12.34 dollars' );
t_eq( 1230, Money::minor_from_decimal( '12.3', 'USD' ), 'money 12.3 dollars' );
t_eq( 1200, Money::minor_from_decimal( '12', 'USD' ), 'money 12 dollars' );
t_eq( 1234, Money::minor_from_decimal( '1234', 'JPY' ), 'money 1234 yen' );
foreach ( array( '', '12.345', '-1', '1e3', ' 1', '1,5', '.5', '5.', '1234567890123456' ) as $bad ) {
	t_refused( static function () use ( $bad ) {
		Money::minor_from_decimal( $bad, 'USD' );
	}, 'Amount', 'money ' . json_encode( $bad ) . ' is refused' );
}
t_refused( static function () {
	Money::minor_from_decimal( '12.5', 'JPY' );
}, 'Amount', 'money yen has no decimals' );
t_eq( '0.001', Money::btc_text( 100000 ), 'btc 100000 sats' );
t_eq( '1', Money::btc_text( 100000000 ), 'btc 1 bitcoin' );
t_eq( '0.00000001', Money::btc_text( 1 ), 'btc 1 sat' );
t_eq( '21000000', Money::btc_text( 2100000000000000 ), 'btc the most' );
t_eq( '0.12345678', Money::btc_text( 12345678 ), 'btc eight decimals' );
t_done();
