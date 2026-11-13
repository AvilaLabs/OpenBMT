<?php
/**
 * WO-41: the texts the customer or the owner sees, each as a literal call
 * WordPress can extract, keyed as in src/Words.php. This is the only file
 * that holds a text. Each is a placeholder until the words are written
 * under docs/SCREEN-RULES.md.
 *
 * @package OpenBmtPay
 */

defined( 'ABSPATH' ) || exit;

final class OpenBmt_Pay_Words {

	/** @var array<string,string>|null */
	private static $texts = null;

	/**
	 * Every text, translated.
	 *
	 * @return array<string,string>
	 */
	public static function all(): array {
		if ( null === self::$texts ) {
			self::$texts = array(
				'WO_01_GATEWAY_TITLE' => __( 'WORDS_NEEDED_WO_01_GATEWAY_TITLE', 'openbmt-pay' ),
				'WO_01_GATEWAY_DESCRIPTION' => __( 'WORDS_NEEDED_WO_01_GATEWAY_DESCRIPTION', 'openbmt-pay' ),
				'WO_01_SETTINGS_ENABLE' => __( 'WORDS_NEEDED_WO_01_SETTINGS_ENABLE', 'openbmt-pay' ),
				'WO_01_SETTINGS_HEADING' => __( 'WORDS_NEEDED_WO_01_SETTINGS_HEADING', 'openbmt-pay' ),
				'WO_05_PAIRING_LABEL' => __( 'WORDS_NEEDED_WO_05_PAIRING_LABEL', 'openbmt-pay' ),
				'WO_05_PAIRING_SAVE' => __( 'WORDS_NEEDED_WO_05_PAIRING_SAVE', 'openbmt-pay' ),
				'WO_05_PAIRING_REFUSED' => __( 'WORDS_NEEDED_WO_05_PAIRING_REFUSED', 'openbmt-pay' ),
				'WO_05_PAIRED_AS' => __( 'WORDS_NEEDED_WO_05_PAIRED_AS', 'openbmt-pay' ),
				'WO_05_FINGERPRINT' => __( 'WORDS_NEEDED_WO_05_FINGERPRINT', 'openbmt-pay' ),
				'WO_06_HTTPS_REQUIRED' => __( 'WORDS_NEEDED_WO_06_HTTPS_REQUIRED', 'openbmt-pay' ),
				'WO_07_UNPAIR' => __( 'WORDS_NEEDED_WO_07_UNPAIR', 'openbmt-pay' ),
				'WO_07_UNPAIRED' => __( 'WORDS_NEEDED_WO_07_UNPAIRED', 'openbmt-pay' ),
				'WO_11_LOW' => __( 'WORDS_NEEDED_WO_11_LOW', 'openbmt-pay' ),
				'WO_11_NONE' => __( 'WORDS_NEEDED_WO_11_NONE', 'openbmt-pay' ),
				'WO_11_LEFT' => __( 'WORDS_NEEDED_WO_11_LEFT', 'openbmt-pay' ),
				'WO_11_DAMAGED' => __( 'WORDS_NEEDED_WO_11_DAMAGED', 'openbmt-pay' ),
				'WO_15_NO_QUOTE' => __( 'WORDS_NEEDED_WO_15_NO_QUOTE', 'openbmt-pay' ),
				'WO_15_CURRENCY_UNSUPPORTED' => __( 'WORDS_NEEDED_WO_15_CURRENCY_UNSUPPORTED', 'openbmt-pay' ),
				'WO_16_TITLE' => __( 'WORDS_NEEDED_WO_16_TITLE', 'openbmt-pay' ),
				'WO_16_ADDRESS' => __( 'WORDS_NEEDED_WO_16_ADDRESS', 'openbmt-pay' ),
				'WO_16_AMOUNT' => __( 'WORDS_NEEDED_WO_16_AMOUNT', 'openbmt-pay' ),
				'WO_16_LINK' => __( 'WORDS_NEEDED_WO_16_LINK', 'openbmt-pay' ),
				'WO_16_TIME_LEFT' => __( 'WORDS_NEEDED_WO_16_TIME_LEFT', 'openbmt-pay' ),
				'WO_16_QUOTE_RAN_OUT' => __( 'WORDS_NEEDED_WO_16_QUOTE_RAN_OUT', 'openbmt-pay' ),
				'WO_16_QR_LABEL' => __( 'WORDS_NEEDED_WO_16_QR_LABEL', 'openbmt-pay' ),
				'WO_17_NOTE_ON_HOLD' => __( 'WORDS_NEEDED_WO_17_NOTE_ON_HOLD', 'openbmt-pay' ),
				'WO_21_LAST_VISIT' => __( 'WORDS_NEEDED_WO_21_LAST_VISIT', 'openbmt-pay' ),
				'WO_21_NEVER_VISITED' => __( 'WORDS_NEEDED_WO_21_NEVER_VISITED', 'openbmt-pay' ),
				'WO_31_NOTE_PAID' => __( 'WORDS_NEEDED_WO_31_NOTE_PAID', 'openbmt-pay' ),
				'WO_31_NOTE_PART_PAID' => __( 'WORDS_NEEDED_WO_31_NOTE_PART_PAID', 'openbmt-pay' ),
				'WO_31_NOTE_PAID_LATE' => __( 'WORDS_NEEDED_WO_31_NOTE_PAID_LATE', 'openbmt-pay' ),
				'WO_32_LOG_HEADING' => __( 'WORDS_NEEDED_WO_32_LOG_HEADING', 'openbmt-pay' ),
				'WO_32_LOG_EMPTY' => __( 'WORDS_NEEDED_WO_32_LOG_EMPTY', 'openbmt-pay' ),
				'WO_32_LOG_LINE' => __( 'WORDS_NEEDED_WO_32_LOG_LINE', 'openbmt-pay' ),
			);
		}
		return self::$texts;
	}

	/**
	 * The text of a key of OpenBmtPay\Words.
	 */
	public static function t( string $key ): string {
		$all = self::all();
		return $all[ $key ] ?? '';
	}
}
