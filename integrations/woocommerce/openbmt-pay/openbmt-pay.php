<?php
/**
 * Plugin Name:       OpenBMT Pay
 * Description:       WORDS_NEEDED_WO_41_PLUGIN_DESCRIPTION
 * Version:           0.1.0
 * Requires PHP:      8.2
 * Requires at least: 6.5
 * Requires Plugins:  woocommerce
 * License:           MIT
 * Text Domain:       openbmt-pay
 *
 * Bitcoin at checkout in the shop's own WooCommerce store (docs/WEB-ORDER-RULES.md).
 * The logic is in src/ and calls no WordPress function; wc/ is the thin layer
 * that talks to WordPress and WooCommerce.
 *
 * @package OpenBmtPay
 */

defined( 'ABSPATH' ) || exit;

define( 'OPENBMT_PAY_DIR', plugin_dir_path( __FILE__ ) );

require_once OPENBMT_PAY_DIR . 'src/load.php';
require_once OPENBMT_PAY_DIR . 'wc/class-openbmt-pay-text.php';
require_once OPENBMT_PAY_DIR . 'wc/class-openbmt-pay-store.php';
require_once OPENBMT_PAY_DIR . 'wc/class-openbmt-pay-rates.php';
require_once OPENBMT_PAY_DIR . 'wc/class-openbmt-pay-rest.php';
require_once OPENBMT_PAY_DIR . 'wc/class-openbmt-pay-admin.php';

// Orders are kept the way WooCommerce keeps them, with its high-performance storage or without.
add_action(
	'before_woocommerce_init',
	static function () {
		if ( class_exists( \Automattic\WooCommerce\Utilities\FeaturesUtil::class ) ) {
			\Automattic\WooCommerce\Utilities\FeaturesUtil::declare_compatibility( 'custom_order_tables', __FILE__, true );
		}
	}
);

add_action(
	'plugins_loaded',
	static function () {
		if ( ! class_exists( 'WC_Payment_Gateway' ) ) {
			return;
		}
		require_once OPENBMT_PAY_DIR . 'wc/class-openbmt-pay-gateway.php';
		add_filter(
			'woocommerce_payment_gateways',
			static function ( $gateways ) {
				$gateways[] = 'OpenBmt_Pay_Gateway';
				return $gateways;
			}
		);
		OpenBmt_Pay_Rest::register();
		OpenBmt_Pay_Admin::register();
	}
);
