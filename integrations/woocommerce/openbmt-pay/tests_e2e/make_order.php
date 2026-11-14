<?php
/**
 * Makes an order the way the gateway's process_payment does once it has a
 * quote: the next address of the pool is taken for the order under the
 * store's lock, the order's fields are written and it is put on hold. The
 * quote itself (a call to the shop's rate sources) cannot be made without a
 * network, so the amount in satoshis is an argument.
 *
 * Usage: make_order.php <order number> <sats>
 * With a third argument, that address is the order's, as for a store that
 * reports an address it was never given (WO-22). Prints the address.
 *
 * @package OpenBmtPay
 */

declare(strict_types=1);

require_once __DIR__ . '/load_plugin.php';

$number = (int) ( $argv[1] ?? 0 );
$sats   = (int) ( $argv[2] ?? 0 );
$taken  = isset( $argv[3] )
	? array( 0, (string) $argv[3] )
	: OpenBmt_Pay_Store::with_lock(
		static function () use ( $number ) {
			$pool = OpenBmt_Pay_Store::health()['pool'];
			$took = null === $pool ? null : $pool->take( $number );
			if ( null !== $took ) {
				OpenBmt_Pay_Store::save_pool( $pool );
			}
			return $took;
		}
	);
if ( ! is_array( $taken ) ) {
	fwrite( STDERR, "no address left\n" );
	exit( 1 );
}
$now = time();
$GLOBALS['obm_e2e_orders'][ (string) $number ] = array(
	'payment_method' => 'openbmt',
	'status'         => 'pending',
	'paid'           => false,
	'notes'          => array(),
	'meta'           => array(),
);
$order = wc_get_order( $number );
$order->update_meta_data( '_openbmt_seq', (string) $number );
$order->update_meta_data( '_openbmt_index', (string) $taken[0] );
$order->update_meta_data( '_openbmt_address', $taken[1] );
$order->update_meta_data( '_openbmt_sats', (string) $sats );
$order->update_meta_data( '_openbmt_quoted_at', (string) $now );
$order->update_meta_data( '_openbmt_expires_at', (string) ( $now + 1800 ) );
$order->update_meta_data( '_openbmt_total_minor', '2500' );
$order->update_meta_data( '_openbmt_currency', 'USD' );
$order->update_status( 'on-hold', 'on hold' );
echo $taken[1], "\n";
