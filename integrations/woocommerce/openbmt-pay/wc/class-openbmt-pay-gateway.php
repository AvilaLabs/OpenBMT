<?php
/**
 * The payment gateway: Bitcoin at checkout (WO-01, WO-15 to WO-18). The
 * order is set on hold with a note; only a signed notice from the owner phone
 * marks it paid (WO-01, WO-31). Stock is left to WooCommerce's own handling of
 * an order on hold (WO-17).
 *
 * @package OpenBmtPay
 */

defined( 'ABSPATH' ) || exit;

use OpenBmtPay\Health;
use OpenBmtPay\Money;
use OpenBmtPay\PaymentUri;
use OpenBmtPay\Qr;
use OpenBmtPay\Quote;
use OpenBmtPay\Rates;
use OpenBmtPay\Refusal;

class OpenBmt_Pay_Gateway extends WC_Payment_Gateway {

	/** The tags and attributes of the code's SVG (Qr::svg). */
	const SVG_ALLOWED = array(
		'svg'  => array(
			'xmlns'           => true,
			'viewbox'         => true,
			'shape-rendering' => true,
		),
		'rect' => array(
			'width'  => true,
			'height' => true,
			'fill'   => true,
		),
		'path' => array(
			'fill' => true,
			'd'    => true,
		),
	);

	public function __construct() {
		$this->id                 = 'openbmt';
		$this->icon               = '';
		$this->has_fields         = false;
		$this->method_title       = OpenBmt_Pay_Words::t( 'WO_01_GATEWAY_TITLE' );
		$this->method_description = OpenBmt_Pay_Words::t( 'WO_01_GATEWAY_DESCRIPTION' );
		$this->supports           = array( 'products' );

		$this->init_form_fields();
		$this->init_settings();
		$this->title       = OpenBmt_Pay_Words::t( 'WO_01_GATEWAY_TITLE' );
		$this->description = OpenBmt_Pay_Words::t( 'WO_01_GATEWAY_DESCRIPTION' );
		$this->enabled     = $this->get_option( 'enabled' );

		add_action( 'woocommerce_update_options_payment_gateways_' . $this->id, array( $this, 'process_admin_options' ) );
		add_action( 'woocommerce_thankyou_' . $this->id, array( $this, 'payment_page' ) );
	}

	public function init_form_fields() {
		$this->form_fields = array(
			'enabled' => array(
				'title'   => OpenBmt_Pay_Words::t( 'WO_01_SETTINGS_ENABLE' ),
				'type'    => 'checkbox',
				'label'   => OpenBmt_Pay_Words::t( 'WO_01_SETTINGS_ENABLE' ),
				'default' => 'no',
			),
		);
	}

	/**
	 * WO-07, WO-11, WO-15: Bitcoin is hidden at checkout when the store is
	 * not paired, has no address left, is not on HTTPS, uses a currency with
	 * no price, or cannot make a quote of the cart now.
	 */
	public function is_available() {
		if ( ! parent::is_available() || null === OpenBmt_Pay_Store::pairing() || ! is_ssl() ) {
			return false;
		}
		// Damaged data and an empty pool both hide Bitcoin; damaged data is never read as an empty pool.
		if ( ! Health::shows_bitcoin( OpenBmt_Pay_Store::health() ) ) {
			return false;
		}
		$currency = get_woocommerce_currency();
		if ( null === Rates::band( $currency ) ) {
			return false;
		}
		if ( ! function_exists( 'WC' ) || ! WC()->cart ) {
			return true;
		}
		try {
			$this->quote( $currency, (string) WC()->cart->get_total( 'edit' ) );
		} catch ( Refusal $e ) {
			return false;
		}
		return true;
	}

	/**
	 * WO-15: a quote of an amount in the store's currency.
	 *
	 * @return array{sats:int,rate:int,label:string,quoted_at:int,expires_at:int}
	 * @throws Refusal When no quote can be made.
	 */
	private function quote( string $currency, string $total ): array {
		$minor = Money::minor_from_decimal( wc_format_decimal( $total, Rates::minor_digits( $currency ) ), $currency );
		return Quote::make( $currency, $minor, OpenBmt_Pay_Rates::readings( $currency ), time() );
	}

	/**
	 * WO-10, WO-15, WO-17: takes the order's address and quote, records them,
	 * sets the order on hold with a note. An address taken is never given
	 * again, whatever happens to the order.
	 *
	 * @param int $order_id The order.
	 * @return array<string,string>|null The result, or null on a failure.
	 */
	public function process_payment( $order_id ) {
		$order    = wc_get_order( $order_id );
		$currency = $order ? $order->get_currency() : '';
		try {
			if ( ! $order || null === OpenBmt_Pay_Store::pairing() ) {
				throw new Refusal( 'Unpaired' );
			}
			$quote = $this->quote( $currency, (string) $order->get_total( 'edit' ) );
		} catch ( Refusal $e ) {
			wc_add_notice( OpenBmt_Pay_Words::t( 'WO_15_NO_QUOTE' ), 'error' );
			return null;
		}
		$taken = OpenBmt_Pay_Store::with_lock(
			static function () use ( $order ) {
				$pool = OpenBmt_Pay_Store::health()['pool'];
				if ( null === $pool ) {
					return null;
				}
				$taken = $pool->take( $order->get_id() );
				if ( null !== $taken ) {
					OpenBmt_Pay_Store::save_pool( $pool );
				}
				return $taken;
			}
		);
		if ( ! is_array( $taken ) ) {
			wc_add_notice( OpenBmt_Pay_Words::t( 'WO_15_NO_QUOTE' ), 'error' );
			return null;
		}
		$order->update_meta_data( '_openbmt_seq', (string) $order->get_id() );
		$order->update_meta_data( '_openbmt_index', (string) $taken[0] );
		$order->update_meta_data( '_openbmt_address', $taken[1] );
		$order->update_meta_data( '_openbmt_sats', (string) $quote['sats'] );
		$order->update_meta_data( '_openbmt_quoted_at', (string) $quote['quoted_at'] );
		$order->update_meta_data( '_openbmt_expires_at', (string) $quote['expires_at'] );
		$order->update_meta_data( '_openbmt_total_minor', (string) Money::minor_from_decimal( wc_format_decimal( (string) $order->get_total( 'edit' ), Rates::minor_digits( $currency ) ), $currency ) );
		$order->update_meta_data( '_openbmt_currency', $currency );
		$order->save();
		$order->update_status( 'on-hold', OpenBmt_Pay_Words::t( 'WO_17_NOTE_ON_HOLD' ) );
		return array(
			'result'   => 'success',
			'redirect' => $this->get_return_url( $order ),
		);
	}

	/**
	 * WO-16: the payment page, shown after "Place order": the link with the
	 * address, the amount and the shop's name as label, the amount as text
	 * and the time left on the quote. The page is served by the store.
	 *
	 * @param int $order_id The order.
	 */
	public function payment_page( $order_id ) {
		$order   = wc_get_order( $order_id );
		$pairing = OpenBmt_Pay_Store::pairing();
		if ( ! $order || 'openbmt' !== $order->get_payment_method() || null === $pairing || $order->is_paid() ) {
			return;
		}
		$address = (string) $order->get_meta( '_openbmt_address' );
		$sats    = (int) $order->get_meta( '_openbmt_sats' );
		$left    = max( 0, (int) $order->get_meta( '_openbmt_expires_at' ) - time() );
		$link    = PaymentUri::link( $address, $sats, $pairing['name'] );
		echo '<section class="openbmt-pay">';
		echo '<h2>' . esc_html( OpenBmt_Pay_Words::t( 'WO_16_TITLE' ) ) . '</h2>';
		echo '<p>' . esc_html( OpenBmt_Pay_Words::t( 'WO_16_ADDRESS' ) ) . ' <code>' . esc_html( $address ) . '</code></p>';
		echo '<p>' . esc_html( OpenBmt_Pay_Words::t( 'WO_16_AMOUNT' ) ) . ' <code>' . esc_html( Money::btc_text( $sats ) ) . '</code></p>';
		echo '<p><a href="' . esc_url( $link, array( 'bitcoin' ) ) . '">' . esc_html( OpenBmt_Pay_Words::t( 'WO_16_LINK' ) ) . '</a></p>';
		try {
			$svg = Qr::svg( Qr::encode( PaymentUri::qr_text( $address, $sats, $pairing['name'] ), 'M' ) );
			// The SVG is made from the module matrix alone; kses keeps only its own tags and attributes.
			echo '<div class="openbmt-pay-qr" role="img" aria-label="' . esc_attr( OpenBmt_Pay_Words::t( 'WO_16_QR_LABEL' ) ) . '">' . wp_kses( $svg, self::SVG_ALLOWED ) . '</div>';
		} catch ( Refusal $e ) {
			unset( $e );
		}
		if ( $left > 0 ) {
			echo '<p>' . esc_html( sprintf( OpenBmt_Pay_Words::t( 'WO_16_TIME_LEFT' ), (string) intdiv( $left + 59, 60 ) ) ) . '</p>';
		} else {
			echo '<p>' . esc_html( OpenBmt_Pay_Words::t( 'WO_16_QUOTE_RAN_OUT' ) ) . '</p>';
		}
		echo '</section>';
	}
}
