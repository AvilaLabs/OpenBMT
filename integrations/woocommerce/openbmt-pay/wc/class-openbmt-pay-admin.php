<?php
/**
 * The admin page (WO-05, WO-07, WO-11, WO-21, WO-32): paste the pairing code,
 * see its fingerprint, the addresses left, the last visit and the log of what
 * was refused, and unpair. Every action checks the user's capability and a
 * nonce.
 *
 * @package OpenBmtPay
 */

defined( 'ABSPATH' ) || exit;

use OpenBmtPay\Health;
use OpenBmtPay\Pairing;
use OpenBmtPay\Refusal;

final class OpenBmt_Pay_Admin {

	const PAGE       = 'openbmt-pay';
	const CAPABILITY = 'manage_woocommerce';

	public static function register(): void {
		add_action( 'admin_menu', array( __CLASS__, 'menu' ) );
		add_action( 'admin_post_openbmt_pay_pair', array( __CLASS__, 'pair' ) );
		add_action( 'admin_post_openbmt_pay_unpair', array( __CLASS__, 'unpair' ) );
	}

	public static function menu(): void {
		add_submenu_page(
			'woocommerce',
			OpenBmt_Pay_Words::t( 'WO_01_SETTINGS_HEADING' ),
			OpenBmt_Pay_Words::t( 'WO_01_SETTINGS_HEADING' ),
			self::CAPABILITY,
			self::PAGE,
			array( __CLASS__, 'page' )
		);
	}

	private static function back( string $result ): void {
		wp_safe_redirect( add_query_arg( array( 'page' => self::PAGE, 'result' => $result ), admin_url( 'admin.php' ) ) );
		exit;
	}

	/**
	 * WO-05, WO-06: keeps the pairing code that was pasted. A store that is
	 * not on HTTPS refuses it.
	 */
	public static function pair(): void {
		if ( ! current_user_can( self::CAPABILITY ) ) {
			wp_die( '', '', array( 'response' => 403 ) );
		}
		check_admin_referer( 'openbmt_pay_pair' );
		if ( ! is_ssl() ) {
			self::back( 'https' );
		}
		$text = isset( $_POST['openbmt_pay_code'] ) ? sanitize_text_field( wp_unslash( $_POST['openbmt_pay_code'] ) ) : '';
		try {
			OpenBmt_Pay_Store::pair( Pairing::from_text( $text ) );
		} catch ( Refusal $e ) {
			self::back( 'bad' );
		}
		self::back( 'paired' );
	}

	/**
	 * WO-07: forgets the key and the secret.
	 */
	public static function unpair(): void {
		if ( ! current_user_can( self::CAPABILITY ) ) {
			wp_die( '', '', array( 'response' => 403 ) );
		}
		check_admin_referer( 'openbmt_pay_unpair' );
		OpenBmt_Pay_Store::unpair();
		self::back( 'unpaired' );
	}

	public static function page(): void {
		if ( ! current_user_can( self::CAPABILITY ) ) {
			wp_die( '', '', array( 'response' => 403 ) );
		}
		$pairing = OpenBmt_Pay_Store::pairing();
		$result  = isset( $_GET['result'] ) ? sanitize_key( wp_unslash( $_GET['result'] ) ) : ''; // phpcs:ignore WordPress.Security.NonceVerification.Recommended
		$shown   = array(
			'bad'      => 'WO_05_PAIRING_REFUSED',
			'https'    => 'WO_06_HTTPS_REQUIRED',
			'paired'   => 'WO_05_PAIRED_AS',
			'unpaired' => 'WO_07_UNPAIRED',
		);
		echo '<div class="wrap"><h1>' . esc_html( OpenBmt_Pay_Words::t( 'WO_01_SETTINGS_HEADING' ) ) . '</h1>';
		if ( isset( $shown[ $result ] ) ) {
			echo '<div class="notice notice-info"><p>' . esc_html( OpenBmt_Pay_Words::t( $shown[ $result ] ) ) . '</p></div>';
		}
		if ( null === OpenBmtPay\Rates::band( get_woocommerce_currency() ) ) {
			echo '<div class="notice notice-warning"><p>' . esc_html( OpenBmt_Pay_Words::t( 'WO_15_CURRENCY_UNSUPPORTED' ) ) . '</p></div>';
		}
		$health = OpenBmt_Pay_Store::health();
		if ( Health::DAMAGED === $health['state'] ) {
			echo '<div class="notice notice-error"><p>' . esc_html( OpenBmt_Pay_Words::t( 'WO_11_DAMAGED' ) ) . '</p></div>';
		}
		if ( null === $pairing || Health::DAMAGED === $health['state'] ) {
			echo '<form method="post" action="' . esc_url( admin_url( 'admin-post.php' ) ) . '">';
			echo '<input type="hidden" name="action" value="openbmt_pay_pair">';
			wp_nonce_field( 'openbmt_pay_pair' );
			echo '<p><label for="openbmt_pay_code">' . esc_html( OpenBmt_Pay_Words::t( 'WO_05_PAIRING_LABEL' ) ) . '</label></p>';
			echo '<p><textarea id="openbmt_pay_code" name="openbmt_pay_code" rows="4" cols="60" autocomplete="off"></textarea></p>';
			echo '<p><button type="submit" class="button button-primary">' . esc_html( OpenBmt_Pay_Words::t( 'WO_05_PAIRING_SAVE' ) ) . '</button></p>';
			echo '</form></div>';
			return;
		}
		$pool = $health['pool'];
		echo '<p>' . esc_html( sprintf( OpenBmt_Pay_Words::t( 'WO_05_PAIRED_AS' ), $pairing['name'] ) ) . '</p>';
		echo '<p>' . esc_html( OpenBmt_Pay_Words::t( 'WO_05_FINGERPRINT' ) ) . ' <code>' . esc_html( Pairing::fingerprint_of( $pairing['public'] ) ) . '</code></p>';
		echo '<p>' . esc_html( sprintf( OpenBmt_Pay_Words::t( 'WO_11_LEFT' ), (string) $pool->remaining() ) ) . '</p>';
		if ( $pool->is_empty() ) {
			echo '<div class="notice notice-error"><p>' . esc_html( OpenBmt_Pay_Words::t( 'WO_11_NONE' ) ) . '</p></div>';
		} elseif ( $pool->is_low() ) {
			echo '<div class="notice notice-warning"><p>' . esc_html( OpenBmt_Pay_Words::t( 'WO_11_LOW' ) ) . '</p></div>';
		}
		$visit = OpenBmt_Pay_Store::visit();
		if ( null === $visit ) {
			echo '<p>' . esc_html( OpenBmt_Pay_Words::t( 'WO_21_NEVER_VISITED' ) ) . '</p>';
		} else {
			echo '<p>' . esc_html( sprintf( OpenBmt_Pay_Words::t( 'WO_21_LAST_VISIT' ), wp_date( 'Y-m-d H:i', $visit ) ) ) . '</p>';
		}
		echo '<h2>' . esc_html( OpenBmt_Pay_Words::t( 'WO_32_LOG_HEADING' ) ) . '</h2>';
		$lines = OpenBmt_Pay_Store::log_lines();
		if ( array() === $lines ) {
			echo '<p>' . esc_html( OpenBmt_Pay_Words::t( 'WO_32_LOG_EMPTY' ) ) . '</p>';
		} else {
			echo '<ul>';
			foreach ( array_reverse( $lines ) as $line ) {
				echo '<li>' . esc_html( sprintf( OpenBmt_Pay_Words::t( 'WO_32_LOG_LINE' ), wp_date( 'Y-m-d H:i', $line['time'] ), $line['reason'], null === $line['order'] ? '' : (string) $line['order'] ) ) . '</li>';
			}
			echo '</ul>';
		}
		echo '<form method="post" action="' . esc_url( admin_url( 'admin-post.php' ) ) . '">';
		echo '<input type="hidden" name="action" value="openbmt_pay_unpair">';
		wp_nonce_field( 'openbmt_pay_unpair' );
		echo '<p><button type="submit" class="button">' . esc_html( OpenBmt_Pay_Words::t( 'WO_07_UNPAIR' ) ) . '</button></p>';
		echo '</form></div>';
	}
}
