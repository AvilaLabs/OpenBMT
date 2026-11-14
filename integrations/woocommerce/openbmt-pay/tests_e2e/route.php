<?php
/**
 * One request to the plugin's REST routes, as WordPress would make it: the
 * request is read as JSON on standard input
 * ({"method":"POST","route":"/openbmt/v1/range","headers":{...},"body":"..."}),
 * the routes the plugin registers are looked up, the route's permission
 * callback and then its callback are called, and the answer is printed as
 * {"status":N,"data":...}. It dispatches to the plugin's own callbacks and
 * decides nothing itself (scripts/web-orders-e2e-core.sh).
 *
 * @package OpenBmtPay
 */

declare(strict_types=1);

require_once __DIR__ . '/load_plugin.php';

$request = json_decode( (string) stream_get_contents( STDIN ), true, 8, JSON_THROW_ON_ERROR );
OpenBmt_Pay_Rest::routes();
$route = $GLOBALS['obm_e2e_routes'][ $request['route'] ] ?? null;
if ( null === $route ) {
	echo json_encode( array( 'status' => 404, 'data' => null ) ), "\n";
	exit( 0 );
}
$wp_request = new WP_REST_Request( $request['method'], $request['route'], $request['headers'], $request['body'] );
$allowed    = call_user_func( $route['permission_callback'], $wp_request );
if ( $allowed instanceof WP_Error ) {
	echo json_encode( array( 'status' => $allowed->data['status'] ?? 403, 'data' => null ) ), "\n";
	exit( 0 );
}
$response = call_user_func( $route['callback'], $wp_request );
echo json_encode( array( 'status' => $response->status, 'data' => $response->data ) ), "\n";
