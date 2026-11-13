<?php
/**
 * WO-41: the keys of every text the customer or the owner sees. The texts
 * are in wc/class-openbmt-pay-words.php, each as a literal call WordPress can
 * extract; this file holds no text and calls no WordPress function.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Words {

	/** The keys, in the order of the words file. */
	const KEYS = array(
		'WO_01_GATEWAY_TITLE',
		'WO_01_GATEWAY_DESCRIPTION',
		'WO_01_SETTINGS_ENABLE',
		'WO_01_SETTINGS_HEADING',
		'WO_05_PAIRING_LABEL',
		'WO_05_PAIRING_SAVE',
		'WO_05_PAIRING_REFUSED',
		'WO_05_PAIRED_AS',
		'WO_05_FINGERPRINT',
		'WO_06_HTTPS_REQUIRED',
		'WO_07_UNPAIR',
		'WO_07_UNPAIRED',
		'WO_11_LOW',
		'WO_11_NONE',
		'WO_11_LEFT',
		'WO_11_DAMAGED',
		'WO_15_NO_QUOTE',
		'WO_15_CURRENCY_UNSUPPORTED',
		'WO_16_TITLE',
		'WO_16_ADDRESS',
		'WO_16_AMOUNT',
		'WO_16_LINK',
		'WO_16_TIME_LEFT',
		'WO_16_QUOTE_RAN_OUT',
		'WO_16_QR_LABEL',
		'WO_17_NOTE_ON_HOLD',
		'WO_21_LAST_VISIT',
		'WO_21_NEVER_VISITED',
		'WO_31_NOTE_PAID',
		'WO_31_NOTE_PART_PAID',
		'WO_31_NOTE_PAID_LATE',
		'WO_32_LOG_HEADING',
		'WO_32_LOG_EMPTY',
		'WO_32_LOG_LINE',
	);

	/**
	 * Whether a key is one of the list.
	 */
	public static function has( string $key ): bool {
		return in_array( $key, self::KEYS, true );
	}
}
