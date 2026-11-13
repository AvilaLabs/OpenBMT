<?php
/**
 * The shop's rate sources, fetched by the store: the only call to any server
 * but the store's own (WO-40). A reading is the source, the time the reply
 * arrived by the store's clock, and the text of the price.
 *
 * @package OpenBmtPay
 */

defined( 'ABSPATH' ) || exit;

use OpenBmtPay\Rates;
use OpenBmtPay\Refusal;
use OpenBmtPay\ReplyReader;

final class OpenBmt_Pay_Rates {

	/** The most bytes of a reply that are read (RT-10). */
	const MAX_BYTES = 65536;

	/** The seconds after which a fetch is given up (RT-43). */
	const TIMEOUT_SECS = 20;

	/** The seconds the readings are kept, so a page of the store does not fetch each time. */
	const KEEP_SECS = 60;

	/**
	 * The readings of the sources of a currency. A source that does not
	 * answer, answers with another status than 200 or whose reply is refused
	 * gives none.
	 *
	 * @return array<int,array{0:string,1:int,2:string}>
	 */
	public static function readings( string $currency ): array {
		$key  = 'openbmt_pay_readings_' . $currency;
		$kept = get_transient( $key );
		if ( is_array( $kept ) ) {
			return $kept;
		}
		$out = array();
		foreach ( Rates::sources_for( $currency ) as $source ) {
			$reply = wp_remote_get(
				$source[3],
				array(
					'timeout'             => self::TIMEOUT_SECS,
					'redirection'         => 0,
					'limit_response_size' => self::MAX_BYTES + 1,
					'headers'             => array( 'Accept' => 'application/json' ),
					'cookies'             => array(),
				)
			);
			if ( is_wp_error( $reply ) || 200 !== (int) wp_remote_retrieve_response_code( $reply ) ) {
				continue;
			}
			$body = wp_remote_retrieve_body( $reply );
			if ( strlen( $body ) > self::MAX_BYTES ) {
				continue;
			}
			try {
				$out[] = array( $source[0], time(), ReplyReader::price_text( $source, $body ) );
			} catch ( Refusal $e ) {
				continue;
			}
		}
		set_transient( $key, $out, self::KEEP_SECS );
		return $out;
	}
}
