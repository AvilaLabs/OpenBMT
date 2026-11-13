<?php
/**
 * WO-50: one HMAC made here on fixed inputs, which the Rust side checks the
 * other way round (`wo_20_a_mac_made_by_the_plugin_is_the_one_made_here`).
 * With OPENBMT_BLESS_WEB_ORDER=1 the file is written again.
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\Bytes;
use OpenBmtPay\RequestChecker;

$secret = '';
for ( $i = 0; $i < 32; $i++ ) {
	$secret .= chr( 0x60 + $i );
}
$nonce  = str_repeat( chr( 0x3c ), 16 );
$body   = "php made \xc3\xa9";
$method = 'POST';
$route  = '/openbmt/v1/notice';
$time   = 1800000123;
$mac    = RequestChecker::mac( $secret, $method, $route, (string) $time, bin2hex( $nonce ), $body );

$text = json_encode(
	array(
		'secret' => bin2hex( $secret ),
		'method' => $method,
		'route'  => $route,
		'time'   => $time,
		'nonce'  => bin2hex( $nonce ),
		'body'   => $body,
		'mac'    => $mac,
	),
	JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRETTY_PRINT
) . "\n";

$path = __DIR__ . '/vectors/php_hmac.json';
if ( false !== getenv( 'OPENBMT_BLESS_WEB_ORDER' ) ) {
	file_put_contents( $path, $text );
}
t_eq( $text, (string) file_get_contents( $path ), 'the committed PHP-made MAC is the one made now' );
t_eq( 64, strlen( $mac ), 'the MAC is 32 bytes in hex' );
t_eq( 32, strlen( Bytes::from_hex( $mac ) ), 'the MAC reads as 32 bytes' );
t_done();
