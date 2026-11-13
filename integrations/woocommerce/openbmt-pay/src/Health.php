<?php
/**
 * What the store's kept data says about whether Bitcoin may be shown at
 * checkout. Data that is damaged is never read as an empty pool: Bitcoin is
 * hidden and the owner is told to pair again.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class Health {

	const UNPAIRED = 'unpaired';

	const DAMAGED = 'damaged';

	const READY = 'ready';

	/**
	 * The state of the kept data.
	 *
	 * @param bool  $paired     Whether there is a pairing.
	 * @param mixed $pool_data  What was stored for the pool; null when nothing was.
	 * @param mixed $nonce_data What was stored for the numbers used once; null when nothing was.
	 * @return array{state:string,pool:?AddressPool,nonces:?NonceLedger}
	 */
	public static function inspect( bool $paired, $pool_data, $nonce_data ): array {
		if ( ! $paired ) {
			return array(
				'state'  => self::UNPAIRED,
				'pool'   => null,
				'nonces' => null,
			);
		}
		$pool   = AddressPool::from_array( $pool_data );
		$nonces = NonceLedger::from_array( $nonce_data );
		return array(
			'state'  => ( null === $pool || null === $nonces ) ? self::DAMAGED : self::READY,
			'pool'   => $pool,
			'nonces' => $nonces,
		);
	}

	/**
	 * WO-11: whether Bitcoin is shown at checkout as far as the kept data
	 * goes: the data is sound and an address is left.
	 *
	 * @param array{state:string,pool:?AddressPool,nonces:?NonceLedger} $health The answer of inspect.
	 */
	public static function shows_bitcoin( array $health ): bool {
		return self::READY === $health['state'] && null !== $health['pool'] && ! $health['pool']->is_empty();
	}
}
