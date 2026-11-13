<?php
/**
 * WO-20: the check of an HMAC-authenticated request: the HMAC-SHA256 under the
 * pairing secret of the line-feed-joined text `OBM-WEB-REQUEST-1`, the method,
 * the route, the time in seconds, the number used once in hex and the SHA-256
 * of the body in hex; then the time window; then the number used once.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class RequestChecker {

	const TAG = 'OBM-WEB-REQUEST-1';

	/** The seconds either side of the store's clock a request's time may be: 5 minutes. */
	const WINDOW_SECS = 300;

	/**
	 * The HMAC the owner phone makes, in hex.
	 */
	public static function mac( string $secret, string $method, string $route, string $time_text, string $nonce_hex, string $body ): string {
		$message = self::TAG . "\n" . $method . "\n" . $route . "\n" . $time_text . "\n" . $nonce_hex . "\n" . hash( 'sha256', $body );
		return hash_hmac( 'sha256', $message, $secret );
	}

	/**
	 * Checks a request. The HMAC is checked first, so a number used once is
	 * kept only for a request that has the secret.
	 *
	 * @param string      $secret    The pairing secret (32 bytes).
	 * @param string      $method    The HTTP method.
	 * @param string      $route     The route.
	 * @param string      $time_text The value of the time header.
	 * @param string      $nonce_hex The value of the nonce header.
	 * @param string      $mac_hex   The value of the HMAC header.
	 * @param string      $body      The raw body.
	 * @param int         $now       The store's clock, Unix seconds.
	 * @param NonceLedger $ledger    The numbers used once.
	 * @throws Refusal 'Headers', 'Mac', 'Time', 'Replay' or 'Full'.
	 */
	public static function check( string $secret, string $method, string $route, string $time_text, string $nonce_hex, string $mac_hex, string $body, int $now, NonceLedger $ledger ): void {
		if ( 32 !== strlen( $secret )
			|| 1 !== preg_match( '/^[0-9]{1,15}$/', $time_text )
			|| 1 !== preg_match( '/^[0-9a-f]{32}$/', $nonce_hex )
			|| 1 !== preg_match( '/^[0-9a-f]{64}$/', $mac_hex )
			|| 1 !== preg_match( '/^[A-Z]{1,10}$/', $method )
			|| 1 !== preg_match( '/^\/[\x21-\x7e]{0,200}$/', $route )
		) {
			throw new Refusal( 'Headers' );
		}
		if ( ! hash_equals( self::mac( $secret, $method, $route, $time_text, $nonce_hex, $body ), $mac_hex ) ) {
			throw new Refusal( 'Mac' );
		}
		$time = (int) $time_text;
		if ( abs( $now - $time ) > self::WINDOW_SECS ) {
			throw new Refusal( 'Time' );
		}
		$ledger->use_once( $nonce_hex, $time, $now );
	}
}
