<?php
/**
 * The few WordPress and WooCommerce things the plugin's REST and store
 * classes call, kept in a JSON file so that one PHP process per request can
 * play a store (scripts/web-orders-e2e-core.sh). Nothing of the plugin's own
 * logic is here: options are a map, an order is a bag of fields, and the
 * request and response classes only hold what they are given.
 *
 * The state file is named by the environment variable OBM_E2E_STATE. Test
 * only: this is not WordPress and is never part of the plugin that a store
 * installs.
 *
 * @package OpenBmtPay
 */

declare(strict_types=1);

define( 'ABSPATH', __DIR__ . '/' );

$GLOBALS['obm_e2e_file'] = (string) getenv( 'OBM_E2E_STATE' );
if ( '' === $GLOBALS['obm_e2e_file'] ) {
	fwrite( STDERR, "OBM_E2E_STATE is not set\n" );
	exit( 2 );
}
$obm_e2e_text = is_file( $GLOBALS['obm_e2e_file'] ) ? file_get_contents( $GLOBALS['obm_e2e_file'] ) : '';
$obm_e2e_data = '' === $obm_e2e_text ? array() : json_decode( (string) $obm_e2e_text, true, 64, JSON_THROW_ON_ERROR );
$GLOBALS['obm_e2e_options'] = $obm_e2e_data['options'] ?? array();
$GLOBALS['obm_e2e_orders']  = $obm_e2e_data['orders'] ?? array();
$GLOBALS['obm_e2e_routes']  = array();

/** Writes the state back; called once at the end of a request. */
function obm_e2e_save(): void {
	$text = json_encode(
		array(
			'options' => $GLOBALS['obm_e2e_options'],
			'orders'  => $GLOBALS['obm_e2e_orders'],
		),
		JSON_THROW_ON_ERROR | JSON_PRETTY_PRINT
	);
	file_put_contents( $GLOBALS['obm_e2e_file'], $text );
}
register_shutdown_function( 'obm_e2e_save' );

// Options: a map.
function get_option( string $name, $default = false ) {
	return array_key_exists( $name, $GLOBALS['obm_e2e_options'] ) ? $GLOBALS['obm_e2e_options'][ $name ] : $default;
}
function add_option( string $name, $value = '', $deprecated = '', $autoload = true ): bool {
	if ( array_key_exists( $name, $GLOBALS['obm_e2e_options'] ) ) {
		return false;
	}
	$GLOBALS['obm_e2e_options'][ $name ] = $value;
	return true;
}
function update_option( string $name, $value, $autoload = null ): bool {
	$GLOBALS['obm_e2e_options'][ $name ] = $value;
	return true;
}
function delete_option( string $name ): bool {
	unset( $GLOBALS['obm_e2e_options'][ $name ] );
	return true;
}

/**
 * The database object, for the three statements of the store's lock, each
 * with the semantics of MySQL on the options table: INSERT IGNORE changes a
 * row only when the name is free, UPDATE and DELETE change the rows that
 * match the name and the value. A statement is one step that nothing can
 * interleave with, which is what a database gives. Anything else is refused.
 *
 * Tests can set $before, called with the statement before it runs, to play
 * another request between two statements of one request, and read $inserts,
 * the number of INSERT IGNORE statements, as the number of tries at the lock.
 */
final class Obm_E2e_Wpdb {
	public $options = 'wp_options';
	public $inserts = 0;
	public $before  = null;
	/** Quotes the arguments into the %s places, as $wpdb->prepare does. */
	public function prepare( string $query, ...$args ): string {
		$args = array_map(
			static function ( $a ) {
				return "'" . addslashes( (string) $a ) . "'";
			},
			$args
		);
		return vsprintf( $query, $args );
	}
	private function unquote( string $q ): string {
		return stripslashes( substr( $q, 1, -1 ) );
	}
	public function get_var( string $sql ) {
		if ( null !== $this->before ) {
			( $this->before )( $sql );
		}
		if ( 1 !== preg_match( "/^SELECT option_value FROM wp_options WHERE option_name = ('(?:[^'\\\\]|\\\\.)*')$/", $sql, $m ) ) {
			throw new RuntimeException( 'statement not modelled: ' . $sql );
		}
		$name = $this->unquote( $m[1] );
		return array_key_exists( $name, $GLOBALS['obm_e2e_options'] ) ? $GLOBALS['obm_e2e_options'][ $name ] : null;
	}
	public function query( string $sql ) {
		if ( null !== $this->before ) {
			( $this->before )( $sql );
		}
		$q = "('(?:[^'\\\\]|\\\\.)*')";
		if ( 1 === preg_match( "/^INSERT IGNORE INTO wp_options \\(option_name, option_value, autoload\\) VALUES \\($q, $q, 'no'\\)$/", $sql, $m ) ) {
			++$this->inserts;
			$name = $this->unquote( $m[1] );
			if ( array_key_exists( $name, $GLOBALS['obm_e2e_options'] ) ) {
				return 0;
			}
			$GLOBALS['obm_e2e_options'][ $name ] = $this->unquote( $m[2] );
			return 1;
		}
		if ( 1 === preg_match( "/^UPDATE wp_options SET option_value = $q WHERE option_name = $q AND option_value = $q$/", $sql, $m ) ) {
			$name = $this->unquote( $m[2] );
			if ( ( $GLOBALS['obm_e2e_options'][ $name ] ?? null ) !== $this->unquote( $m[3] ) ) {
				return 0;
			}
			$GLOBALS['obm_e2e_options'][ $name ] = $this->unquote( $m[1] );
			return 1;
		}
		if ( 1 === preg_match( "/^DELETE FROM wp_options WHERE option_name = $q AND option_value = $q$/", $sql, $m ) ) {
			$name = $this->unquote( $m[1] );
			if ( ( $GLOBALS['obm_e2e_options'][ $name ] ?? null ) !== $this->unquote( $m[2] ) ) {
				return 0;
			}
			unset( $GLOBALS['obm_e2e_options'][ $name ] );
			return 1;
		}
		throw new RuntimeException( 'statement not modelled: ' . $sql );
	}
}
$GLOBALS['wpdb'] = new Obm_E2e_Wpdb();

function is_ssl(): bool {
	return true;
}
function __( string $text, string $domain = 'default' ): string {
	return $text;
}
function register_rest_route( string $namespace, string $route, array $args ): void {
	$GLOBALS['obm_e2e_routes'][ '/' . $namespace . $route ] = $args;
}

final class WP_Error {
	public $code;
	public $message;
	public $data;
	public function __construct( $code = '', $message = '', $data = '' ) {
		$this->code    = $code;
		$this->message = $message;
		$this->data    = $data;
	}
}

final class WP_REST_Response {
	public $data;
	public $status;
	public function __construct( $data = null, int $status = 200 ) {
		$this->data   = $data;
		$this->status = $status;
	}
}

final class WP_REST_Request {
	private $method;
	private $route;
	private $headers;
	private $body;
	/** @param array<string,string> $headers Header names as sent. */
	public function __construct( string $method, string $route, array $headers, string $body ) {
		$this->method  = $method;
		$this->route   = $route;
		$this->body    = $body;
		$this->headers = array();
		foreach ( $headers as $name => $value ) {
			$this->headers[ str_replace( '-', '_', strtolower( $name ) ) ] = $value;
		}
	}
	public function get_method(): string {
		return $this->method;
	}
	public function get_route(): string {
		return $this->route;
	}
	public function get_body(): string {
		return $this->body;
	}
	public function get_header( string $key ) {
		return $this->headers[ str_replace( '-', '_', strtolower( $key ) ) ] ?? null;
	}
}

/** An order: fields in a bag, saved in the state file. */
final class Obm_E2e_Order {
	private $id;
	public function __construct( int $id ) {
		$this->id = $id;
	}
	private function &row(): array {
		return $GLOBALS['obm_e2e_orders'][ (string) $this->id ];
	}
	public function get_id(): int {
		return $this->id;
	}
	public function get_meta( string $key ) {
		$row = &$this->row();
		return $row['meta'][ $key ] ?? '';
	}
	public function update_meta_data( string $key, $value ): void {
		$row                  = &$this->row();
		$row['meta'][ $key ] = $value;
	}
	public function save(): void {
	}
	public function get_currency(): string {
		$row = &$this->row();
		return (string) ( $row['currency'] ?? 'USD' );
	}
	public function get_total( string $context = 'view' ): string {
		$row = &$this->row();
		return (string) ( $row['total'] ?? '0' );
	}
	public function get_payment_method(): string {
		$row = &$this->row();
		return (string) $row['payment_method'];
	}
	public function is_paid(): bool {
		$row = &$this->row();
		return true === ( $row['paid'] ?? false );
	}
	public function payment_complete( $transaction_id = '' ): bool {
		$row                   = &$this->row();
		$row['paid']           = true;
		$row['status']         = 'processing';
		$row['transaction_id'] = (string) $transaction_id;
		return true;
	}
	public function add_order_note( string $note ): void {
		$row            = &$this->row();
		$row['notes'][] = $note;
	}
	public function update_status( string $status, string $note = '' ): void {
		$row           = &$this->row();
		$row['status'] = $status;
		if ( '' !== $note ) {
			$row['notes'][] = $note;
		}
	}
}

function wc_get_order( $id ) {
	return isset( $GLOBALS['obm_e2e_orders'][ (string) $id ] ) ? new Obm_E2e_Order( (int) $id ) : false;
}

/**
 * The query the orders route makes: the payment method, a limit, ascending
 * by id, and one numeric meta key above a number.
 *
 * @param array<string,mixed> $args The query.
 * @return array<int,Obm_E2e_Order>
 */
function wc_get_orders( array $args ): array {
	$out = array();
	$ids = array_map( 'intval', array_keys( $GLOBALS['obm_e2e_orders'] ) );
	sort( $ids );
	foreach ( $ids as $id ) {
		$row = $GLOBALS['obm_e2e_orders'][ (string) $id ];
		if ( isset( $args['payment_method'] ) && $row['payment_method'] !== $args['payment_method'] ) {
			continue;
		}
		$ok = true;
		foreach ( $args['meta_query'] ?? array() as $q ) {
			$have = (float) ( $row['meta'][ $q['key'] ] ?? 0 );
			$ok   = $ok && isset( $row['meta'][ $q['key'] ] ) && $have > (float) $q['value'];
		}
		if ( $ok ) {
			$out[] = new Obm_E2e_Order( $id );
		}
		if ( count( $out ) >= (int) ( $args['limit'] ?? PHP_INT_MAX ) ) {
			break;
		}
	}
	return $out;
}
