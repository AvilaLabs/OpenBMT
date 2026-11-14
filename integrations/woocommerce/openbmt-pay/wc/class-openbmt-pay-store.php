<?php
/**
 * Storage in WordPress options: the pairing, the pool of addresses, the
 * numbers used once, the log of refused notices and the time of the last
 * visit. Nothing else of the owner's wallet is ever held (WO-03).
 *
 * @package OpenBmtPay
 */

defined( 'ABSPATH' ) || exit;

use OpenBmtPay\AddressPool;
use OpenBmtPay\Bytes;
use OpenBmtPay\Health;
use OpenBmtPay\NonceLedger;
use OpenBmtPay\Pairing;

final class OpenBmt_Pay_Store {

	const OPT_PAIRING = 'openbmt_pay_pairing';
	const OPT_POOL    = 'openbmt_pay_pool';
	const OPT_NONCES  = 'openbmt_pay_nonces';
	const OPT_LOG     = 'openbmt_pay_log';
	const OPT_VISIT   = 'openbmt_pay_visit';
	const OPT_LOCK    = 'openbmt_pay_lock';

	/** The most lines the log keeps. */
	const LOG_MAX = 50;

	/** The seconds after which a lock that was never released is taken over. */
	const LOCK_SECS = 30;

	/**
	 * Runs a step with the store's lock held, so that two requests do not
	 * change the pool, the numbers used once or an order at the same time.
	 *
	 * @param callable $step The step.
	 * @return mixed What the step returns; null when the lock could not be had.
	 */
	public static function with_lock( callable $step ) {
		$mine = null;
		for ( $try = 0; $try < 100; $try++ ) {
			$mine = self::lock_try();
			if ( null !== $mine ) {
				break;
			}
			usleep( 100000 );
		}
		if ( null === $mine ) {
			return null;
		}
		try {
			return $step();
		} finally {
			self::lock_release( $mine );
		}
	}

	/**
	 * One try at the lock. The lock is a row of the options table that one
	 * INSERT IGNORE can create: the request whose insert changed a row holds
	 * it. A lock older than LOCK_SECS is taken over with an UPDATE that only
	 * changes the row while it still has the value that was read, so that two
	 * requests cannot both take it over. The row's value is a random token and
	 * the time.
	 *
	 * @return string|null The value this request wrote, or null when another holds the lock.
	 */
	public static function lock_try(): ?string {
		global $wpdb;
		$mine = bin2hex( random_bytes( 8 ) ) . ':' . time();
		// phpcs:disable WordPress.DB.DirectDatabaseQuery
		$made = $wpdb->query( $wpdb->prepare( "INSERT IGNORE INTO {$wpdb->options} (option_name, option_value, autoload) VALUES (%s, %s, 'no')", self::OPT_LOCK, $mine ) );
		if ( 1 === $made ) {
			return $mine;
		}
		$old = $wpdb->get_var( $wpdb->prepare( "SELECT option_value FROM {$wpdb->options} WHERE option_name = %s", self::OPT_LOCK ) );
		if ( null === $old ) {
			return null;
		}
		$since = (int) substr( (string) $old, (int) strrpos( (string) $old, ':' ) + 1 );
		if ( time() - $since <= self::LOCK_SECS ) {
			return null;
		}
		$took = $wpdb->query( $wpdb->prepare( "UPDATE {$wpdb->options} SET option_value = %s WHERE option_name = %s AND option_value = %s", $mine, self::OPT_LOCK, $old ) );
		// phpcs:enable WordPress.DB.DirectDatabaseQuery
		return 1 === $took ? $mine : null;
	}

	/**
	 * Lets go of the lock, but only while the row still has the value this
	 * request wrote: a request whose lock was taken over leaves the new
	 * holder's lock alone.
	 *
	 * @param string $mine The value lock_try returned.
	 */
	public static function lock_release( string $mine ): void {
		global $wpdb;
		// phpcs:ignore WordPress.DB.DirectDatabaseQuery
		$wpdb->query( $wpdb->prepare( "DELETE FROM {$wpdb->options} WHERE option_name = %s AND option_value = %s", self::OPT_LOCK, $mine ) );
	}

	/**
	 * The pairing, or null when the store is not paired.
	 *
	 * @return array{public:string,secret:string,name:string,paired_at:int}|null Keys and secret as bytes.
	 */
	public static function pairing(): ?array {
		$data = get_option( self::OPT_PAIRING );
		if ( ! is_array( $data ) || ! isset( $data['public'], $data['secret'], $data['name'] ) ) {
			return null;
		}
		try {
			return array(
				'public'    => Bytes::from_hex( (string) $data['public'] ),
				'secret'    => Bytes::from_hex( (string) $data['secret'] ),
				'name'      => (string) $data['name'],
				'paired_at' => (int) ( $data['paired_at'] ?? 0 ),
			);
		} catch ( OpenBmtPay\Refusal $e ) {
			return null;
		}
	}

	/**
	 * WO-05, WO-10: keeps a pairing. A pairing with another id, or none
	 * before, starts with an empty pool of its own and no numbers used once;
	 * only the ranges of that pairing are taken into it.
	 */
	public static function pair( Pairing $code ): void {
		$old = self::pairing();
		if ( null === $old || Pairing::id_of( $old['secret'] ) !== $code->id() || self::health()['state'] === Health::DAMAGED ) {
			update_option( self::OPT_POOL, ( new AddressPool( $code->id() ) )->to_array(), false );
			update_option( self::OPT_NONCES, ( new NonceLedger() )->to_array(), false );
		}
		update_option(
			self::OPT_PAIRING,
			array(
				'public'    => bin2hex( $code->public ),
				'secret'    => bin2hex( $code->secret ),
				'name'      => $code->shop_name,
				'paired_at' => time(),
			),
			false
		);
	}

	/**
	 * WO-07: forgets the key and the secret and the addresses not yet used.
	 */
	public static function unpair(): void {
		delete_option( self::OPT_PAIRING );
		delete_option( self::OPT_POOL );
		delete_option( self::OPT_NONCES );
	}

	/**
	 * The state of the kept data: unpaired, damaged or ready. Data that is
	 * damaged is never read as an empty pool.
	 *
	 * @return array{state:string,pool:?AddressPool,nonces:?NonceLedger}
	 */
	public static function health(): array {
		return Health::inspect( null !== self::pairing(), get_option( self::OPT_POOL, null ), get_option( self::OPT_NONCES, null ) );
	}

	public static function save_pool( AddressPool $pool ): void {
		update_option( self::OPT_POOL, $pool->to_array(), false );
	}

	public static function save_nonces( NonceLedger $ledger ): void {
		update_option( self::OPT_NONCES, $ledger->to_array(), false );
	}

	/**
	 * WO-32: a line in the log of what was refused. The log keeps the last
	 * LOG_MAX lines; each has the time, the reason and the order number.
	 */
	public static function log( string $reason, ?int $order = null ): void {
		$lines   = get_option( self::OPT_LOG, array() );
		$lines   = is_array( $lines ) ? $lines : array();
		$lines[] = array(
			'time'   => time(),
			'reason' => $reason,
			'order'  => $order,
		);
		update_option( self::OPT_LOG, array_slice( $lines, -self::LOG_MAX ), false );
	}

	/**
	 * The log, oldest first.
	 *
	 * @return array<int,array{time:int,reason:string,order:?int}>
	 */
	public static function log_lines(): array {
		$lines = get_option( self::OPT_LOG, array() );
		$out   = array();
		foreach ( is_array( $lines ) ? $lines : array() as $line ) {
			if ( is_array( $line ) && isset( $line['time'], $line['reason'] ) ) {
				$out[] = array(
					'time'   => (int) $line['time'],
					'reason' => (string) $line['reason'],
					'order'  => isset( $line['order'] ) ? (int) $line['order'] : null,
				);
			}
		}
		return $out;
	}

	/**
	 * WO-21: the time of the owner phone's last visit.
	 */
	public static function visit(): ?int {
		$at = get_option( self::OPT_VISIT );
		return is_numeric( $at ) ? (int) $at : null;
	}

	public static function mark_visit(): void {
		update_option( self::OPT_VISIT, time(), false );
	}
}
