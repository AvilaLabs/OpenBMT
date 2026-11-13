<?php
/**
 * WO-30 to WO-32: what a signed notice leads to. The class changes nothing:
 * it returns what the WooCommerce layer should do.
 *
 * The answer is an array with an `action`:
 * - `complete`: mark the order paid with `transaction_id`, and add a note;
 * - `note`: add a note and leave the order on hold;
 * - `none`: change nothing (`reason` is `already_paid` or `repeat`);
 * - `refuse`: change nothing and log (`reason` names why).
 * The array of a `complete` or a `note` carries `key`, which the layer keeps
 * with the order so that the same notice is not noted twice, and the
 * `notice` itself.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class NoticeDecision {

	/**
	 * Decides on the bytes of a notice.
	 *
	 * @param string   $frame  The signed notice.
	 * @param string   $public The owner phone's public key from the pairing.
	 * @param callable $lookup Takes the order number and gives the store's record of the
	 *                         order as array{address:string,paid:bool,last_notice:?string,sats:int},
	 *                         or null when there is no such order.
	 * @return array<string,mixed>
	 */
	public static function decide( string $frame, string $public, callable $lookup ): array {
		try {
			$notice = Notice::open( $frame, $public );
		} catch ( Refusal $e ) {
			return self::refuse( $e->reason );
		}
		$order = $lookup( $notice['order'] );
		if ( ! is_array( $order ) ) {
			return self::refuse( 'UnknownOrder', $notice );
		}
		if ( ! hash_equals( (string) $order['address'], $notice['address'] ) ) {
			return self::refuse( 'Address', $notice );
		}
		if ( ! empty( $order['paid'] ) ) {
			return array(
				'action' => 'none',
				'reason' => 'already_paid',
			);
		}
		$key = self::key( $notice );
		if ( isset( $order['last_notice'] ) && hash_equals( (string) $order['last_notice'], $key ) ) {
			return array(
				'action' => 'none',
				'reason' => 'repeat',
			);
		}
		$base = array(
			'notice'     => $notice,
			'key'        => $key,
			'asked_sats' => (int) $order['sats'],
		);
		if ( 'paid' === $notice['state'] || 'overpaid' === $notice['state'] ) {
			return array_merge(
				array(
					'action'         => 'complete',
					'transaction_id' => $notice['txids'][0],
				),
				$base
			);
		}
		return array_merge( array( 'action' => 'note' ), $base );
	}

	/**
	 * A key for a notice: its state, amount, depth and transactions.
	 *
	 * @param array<string,mixed> $notice An opened notice.
	 */
	public static function key( array $notice ): string {
		return hash( 'sha256', $notice['state'] . '|' . $notice['counted_sats'] . '|' . $notice['depth'] . '|' . implode( ',', $notice['txids'] ) );
	}

	/**
	 * @param string                   $reason Why.
	 * @param array<string,mixed>|null $notice The opened notice, when there is one.
	 * @return array<string,mixed>
	 */
	private static function refuse( string $reason, ?array $notice = null ): array {
		$answer = array(
			'action' => 'refuse',
			'reason' => $reason,
		);
		if ( null !== $notice ) {
			$answer['order'] = $notice['order'];
		}
		return $answer;
	}
}
