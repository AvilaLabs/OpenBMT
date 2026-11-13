<?php
/**
 * RT-10 to RT-17: the reading of a source's reply into the text of its
 * price. Not built in stage 1: the strict reader of the Rust library is not
 * ported yet, so every reply is refused and no quote is made. Bitcoin stays
 * hidden at checkout until it is.
 */

declare(strict_types=1);

namespace OpenBmtPay;

final class ReplyReader {

	/**
	 * The text of the price in a reply.
	 *
	 * @param array{0:string,1:string,2:string,3:string} $source The source row of Rates::TABLE.
	 * @param string                                     $body   The reply.
	 * @throws Refusal Always, 'NotBuilt'.
	 */
	public static function price_text( array $source, string $body ): string {
		unset( $source, $body );
		throw new Refusal( 'NotBuilt' );
	}
}
