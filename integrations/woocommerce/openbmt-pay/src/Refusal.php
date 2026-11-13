<?php
/**
 * A refusal: the reason is a short name, the same as the Rust side's where
 * there is one, and the detail is numbers for a log.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Refusal extends \RuntimeException {

	/** @var string */
	public $reason;

	/** @var array<string,int> */
	public $detail;

	/**
	 * @param string             $reason The short name.
	 * @param array<string,int>  $detail Numbers that go with it.
	 */
	public function __construct( string $reason, array $detail = array() ) {
		parent::__construct( $reason );
		$this->reason = $reason;
		$this->detail = $detail;
	}
}
