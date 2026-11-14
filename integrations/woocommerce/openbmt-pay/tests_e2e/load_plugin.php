<?php
/**
 * Loads the plugin's own classes and the stubs, for the entries of this
 * folder. The plugin's main file is not loaded: it only registers hooks.
 *
 * @package OpenBmtPay
 */

declare(strict_types=1);

require_once __DIR__ . '/wp_stubs.php';
$obm_e2e_plugin = dirname( __DIR__ );
require_once $obm_e2e_plugin . '/src/load.php';
require_once $obm_e2e_plugin . '/wc/class-openbmt-pay-words.php';
require_once $obm_e2e_plugin . '/wc/class-openbmt-pay-store.php';
require_once $obm_e2e_plugin . '/wc/class-openbmt-pay-rest.php';
