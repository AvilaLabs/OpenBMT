<?php
/**
 * WO-16: the BIP 321 link and the text of the code of a web order: the
 * address, the amount in bitcoin and the shop's name as the label (RQ-13),
 * written as CR-83 writes a sale's.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class PaymentUri {

	/**
	 * The link: lower case scheme and address, the amount without trailing
	 * zeros and the label percent-encoded.
	 */
	public static function link( string $address, int $sats, string $label ): string {
		$query = array();
		if ( $sats > 0 ) {
			$query[] = 'amount=' . Money::btc_text( $sats );
		}
		if ( '' !== $label ) {
			$query[] = 'label=' . rawurlencode( $label );
		}
		return 'bitcoin:' . $address . ( array() === $query ? '' : '?' . implode( '&', $query ) );
	}

	/**
	 * The text of the QR code: the scheme and the address in upper case so
	 * that the address is in the alphanumeric set, the rest as in the link.
	 */
	public static function qr_text( string $address, int $sats, string $label ): string {
		$link = self::link( $address, $sats, $label );
		$cut  = strpos( $link, '?' );
		$head = false === $cut ? $link : substr( $link, 0, $cut );
		$tail = false === $cut ? '' : substr( $link, $cut );
		return strtoupper( $head ) . $tail;
	}
}
