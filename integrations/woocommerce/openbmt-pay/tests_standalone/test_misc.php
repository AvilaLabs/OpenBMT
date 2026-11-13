<?php
/**
 * The smaller classes: the address check, the payment link, the words, the
 * pairing name rule and the reply reader that is not built.
 */

declare(strict_types=1);

require_once __DIR__ . '/harness.php';

use OpenBmtPay\Bech32;
use OpenBmtPay\Bytes;
use OpenBmtPay\Pairing;
use OpenBmtPay\PaymentUri;
use OpenBmtPay\Words;

// WO-10: the addresses of BIP 173 and BIP 350 are accepted, a changed one is not.
$good = array(
	'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4',
	'bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3',
	'tb1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3q0sl5k7',
	'bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0',
);
foreach ( $good as $a ) {
	t_ok( Bech32::is_address( $a ), "wo_10 accepts $a" );
	t_ok( ! Bech32::is_address( strtoupper( $a ) ), "wo_10 refuses upper case $a" );
	t_ok( ! Bech32::is_address( $a . 'q' ), "wo_10 refuses a longer $a" );
	for ( $i = 4, $n = strlen( $a ); $i < $n; $i += 5 ) {
		$changed       = $a;
		$changed[ $i ] = 'q' === $a[ $i ] ? 'p' : 'q';
		t_ok( ! Bech32::is_address( $changed ), "wo_10 refuses a changed character $i of $a" );
	}
}
foreach ( array( '', 'bc1', 'bc1q', '1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2', 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4 ', ' bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4', "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4\n", 'xx1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4', str_repeat( 'q', 200 ) ) as $i => $a ) {
	t_ok( ! Bech32::is_address( $a ), "wo_10 refuses non address $i" );
}

// WO-05: names.
t_ok( Pairing::name_is_valid( 'Shop' ), 'wo_05 a plain name' );
t_ok( Pairing::name_is_valid( "Caf\xc3\xa9" ), 'wo_05 a name with an accent' );
t_ok( ! Pairing::name_is_valid( '' ), 'wo_05 an empty name' );
t_ok( ! Pairing::name_is_valid( str_repeat( 'a', 65 ) ), 'wo_05 a long name' );
t_ok( Pairing::name_is_valid( str_repeat( 'a', 64 ) ), 'wo_05 the longest name' );
t_ok( ! Pairing::name_is_valid( "a\nb" ), 'wo_05 a line feed' );
t_ok( ! Pairing::name_is_valid( "a\x7f" ), 'wo_05 a delete byte' );
t_ok( ! Pairing::name_is_valid( "a\xff" ), 'wo_05 a byte that is not UTF-8' );

// Hex and base32.
t_eq( "\x01\xab", Bytes::from_hex( '01ab' ), 'hex reads' );
foreach ( array( '0', 'AB', 'zz', '0x', ' 0' ) as $bad ) {
	t_refused( static function () use ( $bad ) {
		Bytes::from_hex( $bad );
	}, 'Hex', 'hex refuses ' . json_encode( $bad ) );
}
t_eq( 'foobar', Bytes::from_base32( 'MZXW6YTBOI' ), 'base32 reads (RFC 4648)' );
t_refused( static function () {
	Bytes::from_base32( 'MZXW6YTBOJ' );
}, 'Base32', 'base32 refuses loose bits' );
t_refused( static function () {
	Bytes::from_base32( 'mzxw' );
}, 'Base32', 'base32 refuses lower case' );

// WO-16: the link and the code text.
$a = 'bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4';
t_eq( 'bitcoin:' . $a . '?amount=0.001&label=Shop', PaymentUri::link( $a, 100000, 'Shop' ), 'wo_16 the link' );
t_eq( 'bitcoin:' . $a . '?amount=0.001&label=Caf%C3%A9%20%26%20Co', PaymentUri::link( $a, 100000, "Caf\xc3\xa9 & Co" ), 'wo_16 the label is encoded' );
t_eq( 'bitcoin:' . $a, PaymentUri::link( $a, 0, '' ), 'wo_16 no amount and no label leaves the query out' );
t_eq( 'BITCOIN:' . strtoupper( $a ) . '?amount=0.001&label=Shop', PaymentUri::qr_text( $a, 100000, 'Shop' ), 'wo_16 the code text has the address in upper case' );

// WO-41: the keys of the words, and the one file that holds the texts.
t_eq( count( Words::KEYS ), count( array_unique( Words::KEYS ) ), 'wo_41 the keys are distinct' );
t_eq( 34, count( Words::KEYS ), 'wo_41 there are 34 keys' );
$file = (string) file_get_contents( dirname( __DIR__ ) . '/wc/class-openbmt-pay-words.php' );
preg_match_all( "/^\t+'(WO_[0-9A-Z_]+)' => __\( '((?:[^'\\\\]|\\\\')+)', 'openbmt-pay' \),$/m", $file, $found, PREG_SET_ORDER );
t_eq( count( Words::KEYS ), count( $found ), 'wo_41 every key has one literal call WordPress can extract' );
$in_file = array();
$texts   = array();
foreach ( $found as $m ) {
	t_ok( false === strpos( $m[2], 'WORDS_NEEDED' ), 'wo_41 the text of ' . $m[1] . ' is written' );
	$texts[ $m[1] ] = $m[2];
	$in_file[]      = $m[1];
}
t_eq( count( $texts ), count( array_unique( $texts ) ), 'wo_41 no two keys share a text' );
// Each text takes the values the layer gives it, no more and no fewer.
$takes = array( 'WO_05_PAIRED_AS' => 1, 'WO_11_LEFT' => 1, 'WO_16_TIME_LEFT' => 1, 'WO_21_LAST_VISIT' => 1, 'WO_31_NOTE_PAID' => 4, 'WO_31_NOTE_PART_PAID' => 4, 'WO_31_NOTE_PAID_LATE' => 4, 'WO_32_LOG_LINE' => 3 );
foreach ( $texts as $key => $text ) {
	preg_match_all( '/%(?:([0-9])\$)?s/', $text, $slots );
	$n = count( array_unique( array_map( 'strval', array_keys( array_flip( array_map( static function ( $i, $p ) { return '' === $p ? 'p' . $i : $p; }, array_keys( $slots[0] ), $slots[1] ) ) ) ) ) );
	t_eq( $takes[ $key ] ?? 0, $n, "wo_41 $key takes its values" );
}
t_eq( Words::KEYS, $in_file, 'wo_41 the file has the keys, in the order of the list' );
t_eq( substr_count( $file, '__(' ), count( Words::KEYS ), 'wo_41 the file has no other call' );
foreach ( Words::KEYS as $key ) {
	t_ok( Words::has( $key ), "wo_41 $key is a key" );
	t_ok( 1 === preg_match( '/^WO_[0-9]{2}_[A-Z_]+$/', $key ), "wo_41 $key is named for its rule" );
}
t_ok( ! Words::has( 'WO_99_NOTHING' ), 'wo_41 an unknown key is not a key' );
// No text outside that file and the plugin header: no call of WordPress's translation anywhere else.
$others = array_merge( glob( dirname( __DIR__ ) . '/src/*.php' ), array_diff( glob( dirname( __DIR__ ) . '/wc/*.php' ), array( dirname( __DIR__ ) . '/wc/class-openbmt-pay-words.php' ) ) );
foreach ( $others as $f ) {
	t_ok( 1 !== preg_match( '/\b(__|_e|_x|esc_html__|esc_attr__)\s*\(/', (string) file_get_contents( $f ) ), 'wo_41 ' . basename( $f ) . ' translates nothing itself' );
	t_ok( false === strpos( (string) file_get_contents( $f ), 'WORDS_NEEDED_' ) || 'Words.php' === basename( $f ), 'wo_41 ' . basename( $f ) . ' holds no placeholder text' );
}
// Every key the layer names is in the list, and every key of the list is used somewhere.
$named = array();
foreach ( $others as $f ) {
	if ( 'Words.php' === basename( $f ) ) {
		continue;
	}
	preg_match_all( "/'(WO_[0-9]{2}_[A-Z_]+)'/", (string) file_get_contents( $f ), $asked );
	foreach ( $asked[1] as $key ) {
		t_ok( Words::has( $key ), 'wo_41 ' . basename( $f ) . ' names a key of the list: ' . $key );
		$named[ $key ] = true;
	}
}
foreach ( Words::KEYS as $key ) {
	t_ok( isset( $named[ $key ] ), "wo_41 $key is used by the layer" );
}

// WO-40: no call that reaches outside, no eval, no include of anything remote.
$banned = array( 'eval(', 'curl_', 'file_get_contents( \'http', 'fsockopen', 'shell_exec', 'exec(', 'system(', 'passthru', 'popen', 'proc_open', 'unserialize', 'move_uploaded_file', '$_FILES' );
$files  = array_merge( glob( dirname( __DIR__ ) . '/src/*.php' ), glob( dirname( __DIR__ ) . '/wc/*.php' ), array( dirname( __DIR__ ) . '/openbmt-pay.php' ) );
t_ok( count( $files ) >= 20, 'wo_40 the code files are found' );
foreach ( $files as $file ) {
	$code = (string) file_get_contents( $file );
	foreach ( $banned as $word ) {
		t_ok( false === stripos( $code, $word ), 'wo_40 ' . basename( $file ) . ' has no ' . $word );
	}
	t_ok( 1 !== preg_match( '/\b(include|require)(_once)?\s*\(?\s*[\'"]https?:/i', $code ), 'wo_40 ' . basename( $file ) . ' includes nothing remote' );
	t_ok( 1 !== preg_match( '/^\s*<\?php/', $code ) || 1 === preg_match( '/^<\?php/', $code ), 'wo_40 ' . basename( $file ) . ' starts with the PHP tag' );
}
t_done();
