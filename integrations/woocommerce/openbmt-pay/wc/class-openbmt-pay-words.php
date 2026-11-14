<?php
/**
 * WO-41: the texts the customer or the owner sees, each as a literal call
 * WordPress can extract, keyed as in src/Words.php. This is the only file
 * that holds a text, written under docs/SCREEN-RULES.md.
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
				'WO_01_GATEWAY_TITLE' => __( 'Bitcoin', 'openbmt-pay' ),
				'WO_01_GATEWAY_DESCRIPTION' => __( 'Pay from your own bitcoin wallet. The order goes ahead once the payment is confirmed on the Bitcoin network.', 'openbmt-pay' ),
				'WO_01_SETTINGS_ENABLE' => __( 'Offer bitcoin at checkout', 'openbmt-pay' ),
				'WO_01_SETTINGS_HEADING' => __( 'OpenBMT bitcoin payments', 'openbmt-pay' ),
				'WO_05_PAIRING_LABEL' => __( 'Pairing code from the OpenBMT owner phone', 'openbmt-pay' ),
				'WO_05_PAIRING_SAVE' => __( 'Pair', 'openbmt-pay' ),
				'WO_05_PAIRING_REFUSED' => __( 'That pairing code was not accepted. Copy the whole code again from the owner phone and paste it here.', 'openbmt-pay' ),
				'WO_05_PAIRED_AS' => __( 'Paired with the owner phone of %s.', 'openbmt-pay' ),
				'WO_05_PAIR_WHILE_PAIRED' => __( 'WORDS_NEEDED pair while paired', 'openbmt-pay' ),
				'WO_05_FINGERPRINT' => __( 'Key fingerprint, to compare with the one on the owner phone:', 'openbmt-pay' ),
				'WO_06_HTTPS_REQUIRED' => __( 'This store\'s address does not start with https, so it cannot be paired. Turn on HTTPS for the store first.', 'openbmt-pay' ),
				'WO_07_UNPAIR' => __( 'Unpair', 'openbmt-pay' ),
				'WO_07_UNPAIRED' => __( 'Unpaired. Bitcoin is no longer offered at checkout. The owner phone keeps watching the addresses already given.', 'openbmt-pay' ),
				'WO_11_LOW' => __( 'Few bitcoin addresses are left. Open the OpenBMT owner app so it sends more.', 'openbmt-pay' ),
				'WO_11_NONE' => __( 'No bitcoin addresses are left, so bitcoin is not offered at checkout. Open the OpenBMT owner app so it sends more.', 'openbmt-pay' ),
				'WO_11_LEFT' => __( 'Bitcoin addresses left: %s', 'openbmt-pay' ),
				'WO_11_DAMAGED' => __( 'The stored pairing data is damaged, so bitcoin is not offered at checkout. Pair again with a new code from the owner phone.', 'openbmt-pay' ),
				'WO_15_NO_QUOTE' => __( 'Bitcoin cannot be used for this order right now. Please choose another way to pay.', 'openbmt-pay' ),
				'WO_15_CURRENCY_UNSUPPORTED' => __( 'There is no bitcoin price for the store\'s currency, so bitcoin is not offered at checkout.', 'openbmt-pay' ),
				'WO_16_TITLE' => __( 'Pay with bitcoin', 'openbmt-pay' ),
				'WO_16_ADDRESS' => __( 'Address:', 'openbmt-pay' ),
				'WO_16_AMOUNT' => __( 'Amount:', 'openbmt-pay' ),
				'WO_16_LINK' => __( 'Open in your wallet', 'openbmt-pay' ),
				'WO_16_TIME_LEFT' => __( 'Minutes left at this price: %s', 'openbmt-pay' ),
				'WO_16_QUOTE_RAN_OUT' => __( 'The time for this price has run out. If you have not paid yet, contact the shop before you pay.', 'openbmt-pay' ),
				'WO_16_QR_LABEL' => __( 'Bitcoin payment code to scan with your wallet', 'openbmt-pay' ),
				'WO_17_NOTE_ON_HOLD' => __( 'Waiting for the bitcoin payment to be confirmed.', 'openbmt-pay' ),
				'WO_21_LAST_VISIT' => __( 'The owner phone last checked the store: %s', 'openbmt-pay' ),
				'WO_21_NEVER_VISITED' => __( 'The owner phone has not checked the store yet.', 'openbmt-pay' ),
				'WO_31_NOTE_PAID' => __( 'Bitcoin payment confirmed: %1$s received, %2$s asked, %4$s confirmations. Transactions: %3$s', 'openbmt-pay' ),
				'WO_31_NOTE_PART_PAID' => __( 'Part of the bitcoin payment arrived: %1$s received, %2$s asked, %4$s confirmations. The order stays on hold for the owner to decide. Transactions: %3$s', 'openbmt-pay' ),
				'WO_31_NOTE_PAID_LATE' => __( 'Bitcoin payment confirmed after its price ran out: %1$s received, %2$s asked, %4$s confirmations. The order stays on hold for the owner to decide. Transactions: %3$s', 'openbmt-pay' ),
				'WO_32_LOG_HEADING' => __( 'Refused messages', 'openbmt-pay' ),
				'WO_32_LOG_EMPTY' => __( 'No message has been refused.', 'openbmt-pay' ),
				'WO_32_LOG_LINE' => __( '%1$s, refused: %2$s %3$s', 'openbmt-pay' ),
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
