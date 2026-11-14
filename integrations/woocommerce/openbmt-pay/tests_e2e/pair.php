<?php
/**
 * Pairs the store with the code given as the first argument, as the plugin's
 * admin page does once its capability and nonce checks have passed: the code
 * is read by the plugin's Pairing and kept by its store. Prints the key's
 * fingerprint, which the owner compares with the owner phone's.
 *
 * @package OpenBmtPay
 */

declare(strict_types=1);

require_once __DIR__ . '/load_plugin.php';

$code = OpenBmtPay\Pairing::from_text( (string) ( $argv[1] ?? '' ) );
OpenBmt_Pay_Store::pair( $code );
echo OpenBmtPay\Pairing::fingerprint_of( $code->public ), "\n";
