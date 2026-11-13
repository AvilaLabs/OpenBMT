<?php
/**
 * The one place where the words of src/Words.php are passed through
 * WordPress's translation.
 *
 * @package OpenBmtPay
 */

defined( 'ABSPATH' ) || exit;

final class OpenBmt_Pay_Text {

	/**
	 * The translated text of a key of OpenBmtPay\Words.
	 *
	 * @param string $key A key of OpenBmtPay\Words::KEYS.
	 */
	public static function t( string $key ): string {
		// The text is not a literal here: the words file is the only place the texts are written.
		return __( OpenBmtPay\Words::get( $key ), 'openbmt-pay' ); // phpcs:ignore WordPress.WP.I18n.NonSingularStringLiteralText
	}
}
