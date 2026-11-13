<?php
/**
 * The routes of the owner phone's visits: the request for orders (WO-20), a
 * signed range (WO-10) and a signed notice (WO-30). Each route checks the
 * HMAC of the request in its permission callback, before it reads anything
 * of the request but the headers and the raw body.
 *
 * @package OpenBmtPay
 */

defined( 'ABSPATH' ) || exit;

use OpenBmtPay\AddressPool;
use OpenBmtPay\AddressRange;
use OpenBmtPay\Bytes;
use OpenBmtPay\Health;
use OpenBmtPay\Pairing;
use OpenBmtPay\NoticeDecision;
use OpenBmtPay\Refusal;
use OpenBmtPay\RequestChecker;

final class OpenBmt_Pay_Rest {

	const NAMESPACE_NAME = 'openbmt/v1';

	/** The most orders one reply carries. */
	const ORDERS_MAX = 100;

	/** The most bytes of a body that are taken: a range of 100 addresses in hex fits. */
	const BODY_MAX = 32768;

	public static function register(): void {
		add_action( 'rest_api_init', array( __CLASS__, 'routes' ) );
	}

	public static function routes(): void {
		foreach ( array(
			'/orders' => 'orders',
			'/range'  => 'range',
			'/notice' => 'notice',
		) as $route => $method ) {
			register_rest_route(
				self::NAMESPACE_NAME,
				$route,
				array(
					'methods'             => 'POST',
					'callback'            => array( __CLASS__, $method ),
					'permission_callback' => array( __CLASS__, 'authenticate' ),
				)
			);
		}
	}

	/**
	 * WO-20: the HMAC, the time window and the number used once. Nothing is
	 * answered to a request that fails, but the status; the reason goes to
	 * no one.
	 *
	 * @param WP_REST_Request $request The request.
	 * @return true|WP_Error
	 */
	public static function authenticate( $request ) {
		$denied  = new WP_Error( 'openbmt_denied', '', array( 'status' => 403 ) );
		$pairing = OpenBmt_Pay_Store::pairing();
		$body    = $request->get_body();
		if ( null === $pairing || ! is_ssl() || strlen( $body ) > self::BODY_MAX || Health::READY !== OpenBmt_Pay_Store::health()['state'] ) {
			return $denied;
		}
		$result = OpenBmt_Pay_Store::with_lock(
			static function () use ( $request, $pairing, $body ) {
				$ledger = OpenBmt_Pay_Store::health()['nonces'];
				if ( null === $ledger ) {
					return false;
				}
				try {
					RequestChecker::check(
						$pairing['secret'],
						$request->get_method(),
						$request->get_route(),
						(string) $request->get_header( 'x_obm_time' ),
						(string) $request->get_header( 'x_obm_nonce' ),
						(string) $request->get_header( 'x_obm_mac' ),
						$body,
						time(),
						$ledger
					);
				} catch ( Refusal $e ) {
					return false;
				}
				OpenBmt_Pay_Store::save_nonces( $ledger );
				return true;
			}
		);
		return true === $result ? true : $denied;
	}

	/**
	 * WO-20: the orders made since the number in the body, with the number of
	 * addresses left (WO-11).
	 *
	 * @param WP_REST_Request $request The request.
	 */
	public static function orders( $request ): WP_REST_Response {
		$data  = json_decode( $request->get_body(), true, 4 );
		$since = ( is_array( $data ) && isset( $data['since'] ) && is_int( $data['since'] ) && $data['since'] >= 0 ) ? $data['since'] : null;
		if ( null === $since ) {
			return new WP_REST_Response( array( 'error' => 'body' ), 400 );
		}
		$list   = array();
		$orders = wc_get_orders(
			array(
				'payment_method' => 'openbmt',
				'limit'          => self::ORDERS_MAX,
				'orderby'        => 'ID',
				'order'          => 'ASC',
				'meta_query'     => array( // phpcs:ignore WordPress.DB.SlowDBQuery.slow_db_query_meta_query
					array(
						'key'     => '_openbmt_seq',
						'value'   => $since,
						'compare' => '>',
						'type'    => 'NUMERIC',
					),
				),
			)
		);
		foreach ( $orders as $order ) {
			$list[] = array(
				'order'       => $order->get_id(),
				'address'     => (string) $order->get_meta( '_openbmt_address' ),
				'sats'        => (int) $order->get_meta( '_openbmt_sats' ),
				'quoted_at'   => (int) $order->get_meta( '_openbmt_quoted_at' ),
				'total_minor' => (int) $order->get_meta( '_openbmt_total_minor' ),
				'currency'    => (string) $order->get_meta( '_openbmt_currency' ),
			);
		}
		OpenBmt_Pay_Store::mark_visit();
		return new WP_REST_Response(
			array(
				'orders' => $list,
				'left'   => OpenBmt_Pay_Store::health()['pool']->remaining(),
			),
			200
		);
	}

	/**
	 * WO-10: a signed range, in hex. The signature is checked against the key
	 * of the pairing before the pool is touched.
	 *
	 * @param WP_REST_Request $request The request.
	 */
	public static function range( $request ): WP_REST_Response {
		$pairing = OpenBmt_Pay_Store::pairing();
		if ( null === $pairing ) {
			return new WP_REST_Response( array( 'error' => 'unpaired' ), 403 );
		}
		try {
			$range = AddressRange::open( Bytes::from_hex( trim( $request->get_body() ) ), $pairing['public'], Pairing::id_of( $pairing['secret'] ) );
		} catch ( Refusal $e ) {
			OpenBmt_Pay_Store::log( 'range:' . $e->reason );
			return new WP_REST_Response( array( 'error' => 'range' ), 400 );
		}
		$outcome = OpenBmt_Pay_Store::with_lock(
			static function () use ( $range, $pairing ) {
				$pool = OpenBmt_Pay_Store::health()['pool'];
				if ( null === $pool ) {
					return array( Health::DAMAGED, 0 );
				}
				$outcome = $pool->add( $range['start'], $range['addresses'], Pairing::id_of( $pairing['secret'] ) );
				if ( AddressPool::ACCEPTED === $outcome ) {
					OpenBmt_Pay_Store::save_pool( $pool );
				}
				return array( $outcome, $pool->remaining() );
			}
		);
		if ( ! is_array( $outcome ) ) {
			return new WP_REST_Response( array( 'error' => 'busy' ), 503 );
		}
		$ok = in_array( $outcome[0], array( AddressPool::ACCEPTED, AddressPool::STALE ), true );
		if ( ! $ok ) {
			OpenBmt_Pay_Store::log( 'range:' . $outcome[0] );
		}
		return new WP_REST_Response(
			array(
				'result' => $outcome[0],
				'left'   => $outcome[1],
			),
			$ok ? 200 : 409
		);
	}

	/**
	 * WO-30 to WO-32: a signed notice, in hex. The decision is made by
	 * OpenBmtPay\NoticeDecision; this method does what it says and no more.
	 *
	 * @param WP_REST_Request $request The request.
	 */
	public static function notice( $request ): WP_REST_Response {
		$pairing = OpenBmt_Pay_Store::pairing();
		if ( null === $pairing ) {
			return new WP_REST_Response( array( 'error' => 'unpaired' ), 403 );
		}
		try {
			$frame = Bytes::from_hex( trim( $request->get_body() ) );
		} catch ( Refusal $e ) {
			OpenBmt_Pay_Store::log( 'notice:' . $e->reason );
			return new WP_REST_Response( array( 'error' => 'notice' ), 400 );
		}
		$decision = OpenBmt_Pay_Store::with_lock(
			static function () use ( $frame, $pairing ) {
				$decision = NoticeDecision::decide( $frame, $pairing['public'], array( __CLASS__, 'record' ) );
				self::act( $decision );
				return $decision;
			}
		);
		if ( ! is_array( $decision ) ) {
			return new WP_REST_Response( array( 'error' => 'busy' ), 503 );
		}
		$status = ( 'refuse' === $decision['action'] ) ? 400 : 200;
		return new WP_REST_Response( array( 'action' => $decision['action'] ), $status );
	}

	/**
	 * The store's record of an order, for the decision.
	 *
	 * @param int $number The order number.
	 * @return array{address:string,paid:bool,last_notice:?string,sats:int}|null
	 */
	public static function record( int $number ): ?array {
		$order = wc_get_order( $number );
		if ( ! $order || 'openbmt' !== $order->get_payment_method() ) {
			return null;
		}
		$last = (string) $order->get_meta( '_openbmt_last_notice' );
		return array(
			'address'     => (string) $order->get_meta( '_openbmt_address' ),
			'paid'        => $order->is_paid() || '1' === $order->get_meta( '_openbmt_paid' ),
			'last_notice' => '' === $last ? null : $last,
			'sats'        => (int) $order->get_meta( '_openbmt_sats' ),
		);
	}

	/**
	 * WO-31, WO-32: what the decision says to do. The order is never
	 * cancelled here.
	 *
	 * @param array<string,mixed> $decision The answer of NoticeDecision::decide.
	 */
	private static function act( array $decision ): void {
		if ( 'refuse' === $decision['action'] ) {
			OpenBmt_Pay_Store::log( 'notice:' . $decision['reason'], isset( $decision['order'] ) ? (int) $decision['order'] : null );
			return;
		}
		if ( 'none' === $decision['action'] ) {
			return;
		}
		$notice = $decision['notice'];
		$order  = wc_get_order( $notice['order'] );
		if ( ! $order ) {
			return;
		}
		$state = $notice['state'];
		$keys  = array(
			'paid'      => 'WO_31_NOTE_PAID',
			'overpaid'  => 'WO_31_NOTE_PAID',
			'part_paid' => 'WO_31_NOTE_PART_PAID',
			'paid_late' => 'WO_31_NOTE_PAID_LATE',
		);
		$note  = sprintf(
			OpenBmt_Pay_Words::t( $keys[ $state ] ),
			OpenBmtPay\Money::btc_text( $notice['counted_sats'] ),
			OpenBmtPay\Money::btc_text( $decision['asked_sats'] ),
			implode( ', ', $notice['txids'] ),
			$notice['depth']
		);
		$order->update_meta_data( '_openbmt_last_notice', $decision['key'] );
		if ( 'complete' === $decision['action'] ) {
			$order->update_meta_data( '_openbmt_paid', '1' );
			$order->save();
			$order->payment_complete( $decision['transaction_id'] );
			$order->add_order_note( $note );
			return;
		}
		$order->save();
		$order->add_order_note( $note );
	}
}
