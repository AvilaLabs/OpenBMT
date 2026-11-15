package org.openbmt.app

/**
 * GV-10: the words of a place of giving. Where a shop's words name a sale, a customer, a price or a counter, a place of
 * giving has words of its own, kept under the same id as the shop's. An id with no entry here is spoken in the shop's
 * words, so the table can be filled one id at a time. The rules, the checks and the records stay those of a sale.
 */
object PlaceWords {
    /**
     * The words of giving by id, the id being the name the shop's words are listed under. TODO_WORDS_GIVING_TABLE: empty
     * until the words are given.
     */
    val giving: Map<String, SayWords> = emptyMap()

    /** The words to show: [shop] at a shop, and at a place of giving the words under [id] in [table], or [shop] when there are none. */
    fun at(isGiving: Boolean, id: String, shop: SayWords, table: Map<String, SayWords> = giving): SayWords =
        if (isGiving) table[id] ?: shop else shop
}
