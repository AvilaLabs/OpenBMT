<?php
/**
 * The sources of a price, the same fixed table as `crates/openbmt-rates`
 * (RT-01): the id, the letter of a combined label, the currency, the
 * address and the shape of its reply (RT-15). The currencies' minor digits
 * and bands (RT-24) are here too.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Rates {

	/** The sources, in the order of the table. */
	const TABLE = array(
		array(
			'kraken',
			'k',
			'USD',
			'https://api.kraken.com/0/public/Ticker?pair=XBTUSD',
			array( 'kind' => 'kraken' ),
		),
		array(
			'coinbase',
			'c',
			'USD',
			'https://api.coinbase.com/v2/prices/BTC-USD/spot',
			array( 'kind' => 'coinbase' ),
		),
		array(
			'bitstamp',
			'b',
			'USD',
			'https://www.bitstamp.net/api/v2/ticker/btcusd/',
			array( 'kind' => 'last' ),
		),
		array(
			'gemini',
			'g',
			'USD',
			'https://api.gemini.com/v1/pubticker/btcusd',
			array( 'kind' => 'last' ),
		),
		array(
			'kraken-eur',
			'k',
			'EUR',
			'https://api.kraken.com/0/public/Ticker?pair=XBTEUR',
			array( 'kind' => 'kraken' ),
		),
		array(
			'coinbase-eur',
			'c',
			'EUR',
			'https://api.coinbase.com/v2/prices/BTC-EUR/spot',
			array( 'kind' => 'coinbase' ),
		),
		array(
			'bitstamp-eur',
			'b',
			'EUR',
			'https://www.bitstamp.net/api/v2/ticker/btceur/',
			array( 'kind' => 'last' ),
		),
		array(
			'gemini-eur',
			'g',
			'EUR',
			'https://api.gemini.com/v1/pubticker/btceur',
			array( 'kind' => 'last' ),
		),
		array(
			'kraken-gbp',
			'k',
			'GBP',
			'https://api.kraken.com/0/public/Ticker?pair=XBTGBP',
			array( 'kind' => 'kraken' ),
		),
		array(
			'coinbase-gbp',
			'c',
			'GBP',
			'https://api.coinbase.com/v2/prices/BTC-GBP/spot',
			array( 'kind' => 'coinbase' ),
		),
		array(
			'bitstamp-gbp',
			'b',
			'GBP',
			'https://www.bitstamp.net/api/v2/ticker/btcgbp/',
			array( 'kind' => 'last' ),
		),
		array(
			'gemini-gbp',
			'g',
			'GBP',
			'https://api.gemini.com/v1/pubticker/btcgbp',
			array( 'kind' => 'last' ),
		),
		array(
			'kraken-jpy',
			'k',
			'JPY',
			'https://api.kraken.com/0/public/Ticker?pair=XBTJPY',
			array( 'kind' => 'kraken' ),
		),
		array(
			'bitflyer-jpy',
			'f',
			'JPY',
			'https://api.bitflyer.com/v1/ticker?product_code=BTC_JPY',
			array( 'kind' => 'number', 'price' => array( 'ltp' ), 'check' => array( 'at' => array( 'product_code' ), 'equals' => 'BTC_JPY', 'fault' => 'Market' ) ),
		),
		array(
			'kraken-aud',
			'k',
			'AUD',
			'https://api.kraken.com/0/public/Ticker?pair=XBTAUD',
			array( 'kind' => 'kraken' ),
		),
		array(
			'luno-aud',
			'l',
			'AUD',
			'https://api.luno.com/api/1/ticker?pair=XBTAUD',
			array( 'kind' => 'text', 'price' => array( 'last_trade' ), 'check' => array( 'at' => array( 'pair' ), 'equals' => 'XBTAUD', 'fault' => 'Market' ) ),
		),
		array(
			'btcmarkets-aud',
			'm',
			'AUD',
			'https://api.btcmarkets.net/v3/markets/BTC-AUD/ticker',
			array( 'kind' => 'text', 'price' => array( 'lastPrice' ), 'check' => array( 'at' => array( 'marketId' ), 'equals' => 'BTC-AUD', 'fault' => 'Market' ) ),
		),
		array(
			'coinbase-inr',
			'c',
			'INR',
			'https://api.coinbase.com/v2/prices/BTC-INR/spot',
			array( 'kind' => 'coinbase' ),
		),
		array(
			'wazirx-inr',
			'w',
			'INR',
			'https://api.wazirx.com/sapi/v1/ticker/24hr?symbol=btcinr',
			array( 'kind' => 'text', 'price' => array( 'lastPrice' ), 'check' => array( 'at' => array( 'symbol' ), 'equals' => 'btcinr', 'fault' => 'Market' ) ),
		),
		array(
			'upbit-krw',
			'u',
			'KRW',
			'https://api.upbit.com/v1/ticker?markets=KRW-BTC',
			array( 'kind' => 'number', 'price' => array( 0, 'trade_price' ), 'check' => array( 'at' => array( 0, 'market' ), 'equals' => 'KRW-BTC', 'fault' => 'Market' ) ),
		),
		array(
			'bithumb-krw',
			't',
			'KRW',
			'https://api.bithumb.com/public/ticker/BTC_KRW',
			array( 'kind' => 'text', 'price' => array( 'data', 'closing_price' ), 'check' => array( 'at' => array( 'status' ), 'equals' => '0000', 'fault' => 'Status' ) ),
		),
		array(
			'luno-idr',
			'l',
			'IDR',
			'https://api.luno.com/api/1/ticker?pair=XBTIDR',
			array( 'kind' => 'text', 'price' => array( 'last_trade' ), 'check' => array( 'at' => array( 'pair' ), 'equals' => 'XBTIDR', 'fault' => 'Market' ) ),
		),
		array(
			'indodax-idr',
			'd',
			'IDR',
			'https://indodax.com/api/ticker/btcidr',
			array( 'kind' => 'text', 'price' => array( 'ticker', 'last' ), 'check' => null ),
		),
		array(
			'gemini-sgd',
			'g',
			'SGD',
			'https://api.gemini.com/v1/pubticker/btcsgd',
			array( 'kind' => 'last' ),
		),
		array(
			'independentreserve-sgd',
			'i',
			'SGD',
			'https://api.independentreserve.com/Public/GetMarketSummary?primaryCurrencyCode=xbt&secondaryCurrencyCode=sgd',
			array( 'kind' => 'number', 'price' => array( 'LastPrice' ), 'check' => array( 'at' => array( 'SecondaryCurrencyCode' ), 'equals' => 'Sgd', 'fault' => 'Market' ) ),
		),
		array(
			'kraken-cad',
			'k',
			'CAD',
			'https://api.kraken.com/0/public/Ticker?pair=XBTCAD',
			array( 'kind' => 'kraken' ),
		),
		array(
			'ndax-cad',
			'n',
			'CAD',
			'https://api.ndax.io:8443/AP/GetLevel1?OMSId=1&InstrumentId=1',
			array( 'kind' => 'number', 'price' => array( 'LastTradedPx' ), 'check' => null ),
		),
		array(
			'bitso-mxn',
			'b',
			'MXN',
			'https://api.bitso.com/v3/ticker/?book=btc_mxn',
			array( 'kind' => 'text', 'price' => array( 'payload', 'last' ), 'check' => array( 'at' => array( 'payload', 'book' ), 'equals' => 'btc_mxn', 'fault' => 'Market' ) ),
		),
		array(
			'binance-mxn',
			'n',
			'MXN',
			'https://data-api.binance.vision/api/v3/ticker/price?symbol=BTCMXN',
			array( 'kind' => 'text', 'price' => array( 'price' ), 'check' => array( 'at' => array( 'symbol' ), 'equals' => 'BTCMXN', 'fault' => 'Market' ) ),
		),
		array(
			'foxbit-brl',
			'f',
			'BRL',
			'https://api.foxbit.com.br/rest/v3/markets/btcbrl/ticker/24hr',
			array( 'kind' => 'text', 'price' => array( 'data', 0, 'last_trade', 'price' ), 'check' => array( 'at' => array( 'data', 0, 'market_symbol' ), 'equals' => 'btcbrl', 'fault' => 'Market' ) ),
		),
		array(
			'okx-brl',
			'o',
			'BRL',
			'https://www.okx.com/api/v5/market/ticker?instId=BTC-BRL',
			array( 'kind' => 'text', 'price' => array( 'data', 0, 'last' ), 'check' => array( 'at' => array( 'data', 0, 'instId' ), 'equals' => 'BTC-BRL', 'fault' => 'Market' ) ),
		),
		array(
			'binance-brl',
			'n',
			'BRL',
			'https://data-api.binance.vision/api/v3/ticker/price?symbol=BTCBRL',
			array( 'kind' => 'text', 'price' => array( 'price' ), 'check' => array( 'at' => array( 'symbol' ), 'equals' => 'BTCBRL', 'fault' => 'Market' ) ),
		),
		array(
			'btcturk-try',
			't',
			'TRY',
			'https://api.btcturk.com/api/v2/ticker?pairSymbol=BTCTRY',
			array( 'kind' => 'number', 'price' => array( 'data', 0, 'last' ), 'check' => array( 'at' => array( 'data', 0, 'pair' ), 'equals' => 'BTCTRY', 'fault' => 'Market' ) ),
		),
		array(
			'okx-try',
			'o',
			'TRY',
			'https://www.okx.com/api/v5/market/ticker?instId=BTC-TRY',
			array( 'kind' => 'text', 'price' => array( 'data', 0, 'last' ), 'check' => array( 'at' => array( 'data', 0, 'instId' ), 'equals' => 'BTC-TRY', 'fault' => 'Market' ) ),
		),
		array(
			'binance-try',
			'n',
			'TRY',
			'https://data-api.binance.vision/api/v3/ticker/price?symbol=BTCTRY',
			array( 'kind' => 'text', 'price' => array( 'price' ), 'check' => array( 'at' => array( 'symbol' ), 'equals' => 'BTCTRY', 'fault' => 'Market' ) ),
		),
		array(
			'bitkub-thb',
			'k',
			'THB',
			'https://api.bitkub.com/api/v3/market/ticker?sym=BTC_THB',
			array( 'kind' => 'text', 'price' => array( 0, 'last' ), 'check' => array( 'at' => array( 0, 'symbol' ), 'equals' => 'BTC_THB', 'fault' => 'Market' ) ),
		),
		array(
			'binance-thb',
			'n',
			'THB',
			'https://data-api.binance.vision/api/v3/ticker/price?symbol=BTCTHB',
			array( 'kind' => 'text', 'price' => array( 'price' ), 'check' => array( 'at' => array( 'symbol' ), 'equals' => 'BTCTHB', 'fault' => 'Market' ) ),
		),
		array(
			'luno-zar',
			'l',
			'ZAR',
			'https://api.luno.com/api/1/ticker?pair=XBTZAR',
			array( 'kind' => 'text', 'price' => array( 'last_trade' ), 'check' => array( 'at' => array( 'pair' ), 'equals' => 'XBTZAR', 'fault' => 'Market' ) ),
		),
		array(
			'valr-zar',
			'v',
			'ZAR',
			'https://api.valr.com/v1/public/BTCZAR/marketsummary',
			array( 'kind' => 'text', 'price' => array( 'lastTradedPrice' ), 'check' => array( 'at' => array( 'currencyPair' ), 'equals' => 'BTCZAR', 'fault' => 'Market' ) ),
		),
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
	 * The source with this id, as an array of id, letter, currency, address
	 * and shape; null when there is none.
	 *
	 * @return array{0:string,1:string,2:string,3:string,4:array<string,mixed>}|null
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
	 * @return array<int,array{0:string,1:string,2:string,3:string,4:array<string,mixed>}>
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
