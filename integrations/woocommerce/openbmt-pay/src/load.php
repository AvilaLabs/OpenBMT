<?php
/**
 * Loads the plugin's own classes. There is no autoloader and no package
 * manager: the list is the whole of the code in this folder.
 */

declare(strict_types=1);

foreach ( array(
	'Refusal',
	'Bytes',
	'Reader',
	'Bech32',
	'Words',
	'Pairing',
	'AddressRange',
	'AddressPool',
	'Price',
	'Rates',
	'Quote',
	'Money',
	'ReplyReader',
	'PaymentUri',
	'NonceLedger',
	'RequestChecker',
	'Notice',
	'NoticeDecision',
) as $openbmt_pay_class ) {
	require_once __DIR__ . '/' . $openbmt_pay_class . '.php';
}
unset( $openbmt_pay_class );
