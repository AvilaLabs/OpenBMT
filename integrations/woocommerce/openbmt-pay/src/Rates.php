<?php
/**
 * The sources of a price, the same fixed table as `crates/openbmt-rates`
 * (RT-01): the id, the letter of a combined label, the currency and the
 * address. The currencies' minor digits and bands (RT-24) are here too.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Rates {

	/** The sources, in the order of the table. */
	const TABLE = array(
		array( 'kraken', 'k', 'USD', 'https://api.kraken.com/0/public/Ticker?pair=XBTUSD' ),
		array( 'coinbase', 'c', 'USD', 'https://api.coinbase.com/v2/prices/BTC-USD/spot' ),
		array( 'bitstamp', 'b', 'USD', 'https://www.bitstamp.net/api/v2/ticker/btcusd/' ),
		array( 'gemini', 'g', 'USD', 'https://api.gemini.com/v1/pubticker/btcusd' ),
		array( 'kraken-eur', 'k', 'EUR', 'https://api.kraken.com/0/public/Ticker?pair=XBTEUR' ),
		array( 'coinbase-eur', 'c', 'EUR', 'https://api.coinbase.com/v2/prices/BTC-EUR/spot' ),
		array( 'bitstamp-eur', 'b', 'EUR', 'https://www.bitstamp.net/api/v2/ticker/btceur/' ),
		array( 'gemini-eur', 'g', 'EUR', 'https://api.gemini.com/v1/pubticker/btceur' ),
		array( 'kraken-gbp', 'k', 'GBP', 'https://api.kraken.com/0/public/Ticker?pair=XBTGBP' ),
		array( 'coinbase-gbp', 'c', 'GBP', 'https://api.coinbase.com/v2/prices/BTC-GBP/spot' ),
		array( 'bitstamp-gbp', 'b', 'GBP', 'https://www.bitstamp.net/api/v2/ticker/btcgbp/' ),
		array( 'gemini-gbp', 'g', 'GBP', 'https://api.gemini.com/v1/pubticker/btcgbp' ),
		array( 'kraken-jpy', 'k', 'JPY', 'https://api.kraken.com/0/public/Ticker?pair=XBTJPY' ),
		array( 'bitflyer-jpy', 'f', 'JPY', 'https://api.bitflyer.com/v1/ticker?product_code=BTC_JPY' ),
		array( 'kraken-aud', 'k', 'AUD', 'https://api.kraken.com/0/public/Ticker?pair=XBTAUD' ),
		array( 'luno-aud', 'l', 'AUD', 'https://api.luno.com/api/1/ticker?pair=XBTAUD' ),
		array( 'btcmarkets-aud', 'm', 'AUD', 'https://api.btcmarkets.net/v3/markets/BTC-AUD/ticker' ),
		array( 'coinbase-inr', 'c', 'INR', 'https://api.coinbase.com/v2/prices/BTC-INR/spot' ),
		array( 'wazirx-inr', 'w', 'INR', 'https://api.wazirx.com/sapi/v1/ticker/24hr?symbol=btcinr' ),
		array( 'upbit-krw', 'u', 'KRW', 'https://api.upbit.com/v1/ticker?markets=KRW-BTC' ),
		array( 'bithumb-krw', 't', 'KRW', 'https://api.bithumb.com/public/ticker/BTC_KRW' ),
		array( 'luno-idr', 'l', 'IDR', 'https://api.luno.com/api/1/ticker?pair=XBTIDR' ),
		array( 'indodax-idr', 'd', 'IDR', 'https://indodax.com/api/ticker/btcidr' ),
		array( 'gemini-sgd', 'g', 'SGD', 'https://api.gemini.com/v1/pubticker/btcsgd' ),
		array( 'independentreserve-sgd', 'i', 'SGD', 'https://api.independentreserve.com/Public/GetMarketSummary?primaryCurrencyCode=xbt&secondaryCurrencyCode=sgd' ),
		array( 'kraken-cad', 'k', 'CAD', 'https://api.kraken.com/0/public/Ticker?pair=XBTCAD' ),
		array( 'ndax-cad', 'n', 'CAD', 'https://api.ndax.io:8443/AP/GetLevel1?OMSId=1&InstrumentId=1' ),
		array( 'bitso-mxn', 'b', 'MXN', 'https://api.bitso.com/v3/ticker/?book=btc_mxn' ),
		array( 'binance-mxn', 'n', 'MXN', 'https://data-api.binance.vision/api/v3/ticker/price?symbol=BTCMXN' ),
		array( 'foxbit-brl', 'f', 'BRL', 'https://api.foxbit.com.br/rest/v3/markets/btcbrl/ticker/24hr' ),
		array( 'okx-brl', 'o', 'BRL', 'https://www.okx.com/api/v5/market/ticker?instId=BTC-BRL' ),
		array( 'binance-brl', 'n', 'BRL', 'https://data-api.binance.vision/api/v3/ticker/price?symbol=BTCBRL' ),
		array( 'btcturk-try', 't', 'TRY', 'https://api.btcturk.com/api/v2/ticker?pairSymbol=BTCTRY' ),
		array( 'okx-try', 'o', 'TRY', 'https://www.okx.com/api/v5/market/ticker?instId=BTC-TRY' ),
		array( 'binance-try', 'n', 'TRY', 'https://data-api.binance.vision/api/v3/ticker/price?symbol=BTCTRY' ),
		array( 'bitkub-thb', 'k', 'THB', 'https://api.bitkub.com/api/v3/market/ticker?sym=BTC_THB' ),
		array( 'binance-thb', 'n', 'THB', 'https://data-api.binance.vision/api/v3/ticker/price?symbol=BTCTHB' ),
		array( 'luno-zar', 'l', 'ZAR', 'https://api.luno.com/api/1/ticker?pair=XBTZAR' ),
		array( 'valr-zar', 'v', 'ZAR', 'https://api.valr.com/v1/public/BTCZAR/marketsummary' ),
	);

	/** Currencies with no minor unit; every other one has two digits. */
	const NO_MINOR = array( 'JPY', 'KRW' );

	/** The factor of a currency's band over the dollar band. */
	const BAND_FACTOR = array(
		'USD' => 1,
		'EUR' => 1,
		'GBP' => 1,
		'AUD' => 1,
		'SGD' => 1,
		'CAD' => 1,
		'MXN' => 1,
		'BRL' => 1,
		'TRY' => 1,
		'THB' => 1,
		'ZAR' => 1,
		'JPY' => 100,
		'INR' => 100,
		'KRW' => 1000,
		'IDR' => 10000,
	);

	/** The lowest price accepted in dollars, in cents per bitcoin. */
	const MIN_CENTS_PER_BTC = 10000;

	/** The highest price accepted in dollars, in cents per bitcoin. */
	const MAX_CENTS_PER_BTC = 10000000000;

	/**
	 * The source with this id, as an array of id, letter, currency and
	 * address; null when there is none.
	 *
	 * @return array{0:string,1:string,2:string,3:string}|null
	 */
	public static function source( string $id ): ?array {
		foreach ( self::TABLE as $row ) {
			if ( $row[0] === $id ) {
				return $row;
			}
		}
		return null;
	}

	/**
	 * The place of a source in the table.
	 */
	public static function position( string $id ): int {
		foreach ( self::TABLE as $i => $row ) {
			if ( $row[0] === $id ) {
				return $i;
			}
		}
		return count( self::TABLE );
	}

	/**
	 * The sources of a currency, in the order of the table.
	 *
	 * @return array<int,array{0:string,1:string,2:string,3:string}>
	 */
	public static function sources_for( string $currency ): array {
		return array_values(
			array_filter(
				self::TABLE,
				static function ( array $row ) use ( $currency ): bool {
					return $row[2] === $currency;
				}
			)
		);
	}

	/**
	 * The digits after the point of a currency's minor unit.
	 */
	public static function minor_digits( string $currency ): int {
		return in_array( $currency, self::NO_MINOR, true ) ? 0 : 2;
	}

	/**
	 * The lowest and highest price of a currency in minor units per bitcoin;
	 * null for a currency with no price.
	 *
	 * @return array{0:int,1:int}|null
	 */
	public static function band( string $currency ): ?array {
		if ( ! isset( self::BAND_FACTOR[ $currency ] ) ) {
			return null;
		}
		$factor = self::BAND_FACTOR[ $currency ];
		return array( self::MIN_CENTS_PER_BTC * $factor, self::MAX_CENTS_PER_BTC * $factor );
	}
}
