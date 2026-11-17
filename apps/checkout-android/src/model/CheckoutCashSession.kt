package org.openbmt.checkout

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.security.MessageDigest
import java.util.Collections

enum class CashSessionStatus { OPEN, CLOSED }
enum class CashMovementKind { CASH_IN, CASH_OUT }

data class CashMovement(
    val operationId: String,
    val kind: CashMovementKind,
    val amountMinor: Long,
    val reason: String,
    val staffId: String,
    val recordedAt: Long,
)

data class CashSaleLink(
    val saleId: String,
    val paymentId: String,
    /** Applied sale amount, after change. This is the cash added to the drawer. */
    val amountMinor: Long,
    val tenderedMinor: Long,
    val changeMinor: Long,
    val recordedAt: Long,
    /** Null only for records loaded from codec v1/v2, which did not persist the operator label. */
    val staffId: String? = null,
)

data class CashCloseoutSnapshot(
    val countedMinor: Long,
    val expectedMinor: Long,
    /** Positive is over; negative is short. */
    val discrepancyMinor: Long,
    val staffId: String,
    val closedAt: Long,
)

data class CashSession(
    val sessionId: String,
    val drawerId: String,
    val staffId: String,
    val openedAt: Long,
    val openingFloatMinor: Long,
    val status: CashSessionStatus,
    val movements: List<CashMovement>,
    val sales: List<CashSaleLink>,
    val expectedCashMinor: Long,
    val closeout: CashCloseoutSnapshot?,
    /** All session amounts are USD cents; non-USD drawer sessions are not supported. */
    val currency: String = "USD",
)

/**
 * Durable manual cash-session accounting. It records operator-entered counts and movements;
 * it does not observe a physical drawer, prove cash presence, or authenticate the staff label.
 * Sale links are accepted only from a canonical CheckoutLedger cash payment.
 */
class CheckoutCashSessionStore(private val storage: DurableLedgerStorage) {
    private val lock = Any()
    private var state = decode(storage.read())
    private var poisoned = false

    fun sessions(): List<CashSession> = synchronized(lock) {
        healthy()
        immutable(state.sessions.values.map(::view))
    }

    fun getSession(sessionId: String): CashSession = synchronized(lock) {
        healthy(); view(state.sessions[sessionId] ?: error("cash session not found"))
    }

    fun openSession(operationId: String, sessionId: String, drawerId: String, staffId: String,
                    openingFloatMinor: Long, openedAt: Long): CashSession = mutate { next ->
        validId(operationId, "operation id"); validId(sessionId, "session id"); validId(drawerId, "drawer id")
        validId(staffId, "staff id"); nonnegative(openingFloatMinor, "opening float"); nonnegative(openedAt, "opening time")
        val payload = fingerprint("OPEN", sessionId, drawerId, staffId, openingFloatMinor.toString(), openedAt.toString())
        replay(next, operationId, payload)?.let { return@mutate view(next.sessions.getValue(it)) }
        require(next.sessions.size < MAX_SESSIONS) { "cash session limit reached" }
        require(next.sessions.values.none { it.drawerId == drawerId && it.status == CashSessionStatus.OPEN }) {
            "drawer already has an open cash session"
        }
        require(!next.sessions.containsKey(sessionId)) { "cash session id already exists" }
        val data = SessionData(sessionId, drawerId, staffId, openedAt, openingFloatMinor, CashSessionStatus.OPEN,
            mutableListOf(), mutableListOf(), openingFloatMinor, null)
        next.sessions[sessionId] = data
        recordOperation(next, operationId, payload, sessionId)
        view(data)
    }

    fun recordMovement(sessionId: String, operationId: String, kind: CashMovementKind,
                       amountMinor: Long, reason: String, staffId: String, recordedAt: Long): CashSession = mutate { next ->
        validId(sessionId, "session id")
        validId(operationId, "operation id"); validId(staffId, "staff id")
        nonnegative(amountMinor, "movement amount"); require(amountMinor > 0) { "movement amount must be positive" }
        validText(reason, "movement reason"); nonnegative(recordedAt, "movement time")
        val payload = fingerprint("MOVE", sessionId, kind.name, amountMinor.toString(), reason, staffId, recordedAt.toString())
        replay(next, operationId, payload)?.let { return@mutate view(next.sessions.getValue(it)) }
        val s = open(next, sessionId)
        require(s.movements.size < MAX_MOVEMENTS) { "cash movement limit reached" }
        val signed = if (kind == CashMovementKind.CASH_IN) amountMinor else Math.negateExact(amountMinor)
        s.expectedCashMinor = Math.addExact(s.expectedCashMinor, signed)
        require(s.expectedCashMinor >= 0) { "cash out exceeds expected drawer balance" }
        s.movements.add(CashMovement(operationId, kind, amountMinor, reason, staffId, recordedAt))
        recordOperation(next, operationId, payload, sessionId)
        view(s)
    }

    /** Link an already recorded sale payment. Cash is counted by amountMinor (net of change). */
    fun linkCashSale(sessionId: String, operationId: String, saleId: String, paymentId: String,
                     staffId: String, recordedAt: Long, ledger: CheckoutLedger): CashSession = mutate { next ->
        validId(sessionId, "session id")
        validId(operationId, "operation id"); validId(saleId, "sale id"); validId(paymentId, "payment id")
        validId(staffId, "staff id"); nonnegative(recordedAt, "link time")
        val payload = fingerprint("SALE", sessionId, saleId, paymentId, staffId, recordedAt.toString())
        replay(next, operationId, payload)?.let { return@mutate view(next.sessions.getValue(it)) }
        val s = open(next, sessionId)
        val payment = ledger.getSale(saleId).cashPayments.firstOrNull { it.paymentId == paymentId }
            ?: error("canonical cash payment not found for sale")
        require(payment.amountMinor >= 0 && payment.tenderedMinor >= payment.amountMinor &&
            payment.changeMinor == Math.subtractExact(payment.tenderedMinor, payment.amountMinor)) {
            "canonical cash payment has inconsistent change"
        }
        require(next.sessions.values.none { other -> other.sales.any { it.paymentId == paymentId } }) {
            "cash payment is already linked to a session"
        }
        require(s.sales.size < MAX_SALES_PER_SESSION) { "cash sale link limit reached" }
        val link = CashSaleLink(saleId, paymentId, payment.amountMinor, payment.tenderedMinor,
            payment.changeMinor, recordedAt, staffId)
        s.expectedCashMinor = Math.addExact(s.expectedCashMinor, payment.amountMinor)
        s.sales.add(link)
        recordOperation(next, operationId, payload, sessionId)
        view(s)
    }

    fun closeSession(sessionId: String, operationId: String, countedMinor: Long,
                     staffId: String, closedAt: Long): CashSession = mutate { next ->
        validId(sessionId, "session id"); validId(operationId, "operation id"); validId(staffId, "staff id")
        nonnegative(countedMinor, "closing count"); nonnegative(closedAt, "closing time")
        val payload = fingerprint("CLOSE", sessionId, countedMinor.toString(), staffId, closedAt.toString())
        replay(next, operationId, payload)?.let { return@mutate view(next.sessions.getValue(it)) }
        val s = next.sessions[sessionId] ?: error("cash session not found")
        if (s.status == CashSessionStatus.CLOSED) error("cash session is already closed")
        val discrepancy = Math.subtractExact(countedMinor, s.expectedCashMinor)
        s.closeout = CashCloseoutSnapshot(countedMinor, s.expectedCashMinor, discrepancy, staffId, closedAt)
        s.status = CashSessionStatus.CLOSED
        recordOperation(next, operationId, payload, sessionId)
        view(s)
    }

    private inline fun <T> mutate(change: (State) -> T): T = synchronized(lock) {
        healthy()
        val candidate = state.copyDeep()
        val result = change(candidate)
        val encoded = encode(candidate)
        try {
            storage.writeAtomically(encoded)
        } catch (e: Throwable) {
            poisoned = true
            throw IllegalStateException("cash session write outcome is uncertain; reopen before continuing", e)
        }
        state = candidate
        result
    }

    private fun replay(s: State, id: String, payload: String): String? {
        val prior = s.operations[id] ?: return null
        require(prior.payload == payload) { "operation id was already used with different details" }
        return prior.sessionId
    }

    private fun recordOperation(s: State, id: String, payload: String, sessionId: String) {
        require(s.operations.size < MAX_OPERATIONS) { "cash operation limit reached" }
        s.operations[id] = Operation(payload, sessionId)
    }

    private fun open(s: State, id: String): SessionData {
        validId(id, "session id")
        val value = s.sessions[id] ?: error("cash session not found")
        require(value.status == CashSessionStatus.OPEN) { "cash session is closed" }
        return value
    }

    private fun view(s: SessionData) = CashSession(s.id, s.drawerId, s.staffId, s.openedAt, s.openingFloatMinor,
        s.status, immutable(s.movements), immutable(s.sales), s.expectedCashMinor, s.closeout, "USD")

    private fun healthy() { check(!poisoned) { "cash session store has an uncertain write; reopen before continuing" } }

    private class State(val sessions: LinkedHashMap<String, SessionData> = linkedMapOf(),
                        val operations: LinkedHashMap<String, Operation> = linkedMapOf()) {
        fun copyDeep() = State(LinkedHashMap(sessions.mapValues { it.value.copyDeep() }), LinkedHashMap(operations))
    }
    private data class Operation(val payload: String, val sessionId: String)
    private data class SessionData(val id: String, val drawerId: String, val staffId: String, val openedAt: Long,
        val openingFloatMinor: Long, var status: CashSessionStatus, val movements: MutableList<CashMovement>,
        val sales: MutableList<CashSaleLink>, var expectedCashMinor: Long, var closeout: CashCloseoutSnapshot?) {
        fun copyDeep() = copy(movements = movements.toMutableList(), sales = sales.toMutableList())
    }

    private fun decode(bytes: ByteArray?): State {
        if (bytes == null) return State()
        require(bytes.size <= MAX_FILE_BYTES) { "cash session file exceeds size limit" }
        try {
            DataInputStream(ByteArrayInputStream(bytes)).use { input ->
                require(input.readInt() == MAGIC) { "invalid cash session file" }
                val version = input.readInt()
                require(version in 1..VERSION) { "unsupported cash session file version" }
                val state = State()
                repeat(input.count(MAX_SESSIONS)) {
                    val id = input.str(); val drawer = input.str(); val staff = input.str(); val openedAt = input.readLong()
                    val opening = input.readLong(); val status = enumAt<CashSessionStatus>(input.readInt())
                    val expected = input.readLong()
                    require(openedAt >= 0 && opening >= 0 && expected >= 0) { "invalid cash session values" }
                    val movements = mutableListOf<CashMovement>()
                    repeat(input.count(MAX_MOVEMENTS)) {
                        val op = input.str(); val kind = enumAt<CashMovementKind>(input.readInt()); val amount = input.readLong()
                        val reason = input.str(); val actor = input.str(); val at = input.readLong()
                        require(amount > 0 && at >= 0 && reason.isNotBlank()) { "invalid cash movement" }
                        movements.add(CashMovement(op, kind, amount, reason, actor, at))
                    }
                    val sales = mutableListOf<CashSaleLink>()
                    repeat(input.count(MAX_SALES_PER_SESSION)) {
                        val saleId = input.str(); val paymentId = input.str(); val amount = input.readLong()
                        val tender = input.readLong(); val change = input.readLong(); val at = input.readLong()
                        require(amount >= 0 && tender >= amount && change == Math.subtractExact(tender, amount) && at >= 0) { "invalid cash sale link" }
                        val staffId = if (version >= 3 && input.readBoolean()) input.str() else null
                        sales.add(CashSaleLink(saleId, paymentId, amount, tender, change, at, staffId))
                    }
                    val closeout = if (input.readBoolean()) {
                        val counted = input.readLong(); val expectedAtClose = input.readLong(); val discrepancy = input.readLong()
                        val actor = input.str(); val at = input.readLong()
                        require(counted >= 0 && expectedAtClose == expected && discrepancy == Math.subtractExact(counted, expected) && at >= 0) { "invalid closeout snapshot" }
                        CashCloseoutSnapshot(counted, expectedAtClose, discrepancy, actor, at)
                    } else null
                    require((status == CashSessionStatus.CLOSED) == (closeout != null)) { "cash session close state is inconsistent" }
                    val session = SessionData(id, drawer, staff, openedAt, opening, status, movements, sales, expected, closeout)
                    require(state.sessions.put(id, session) == null) { "duplicate cash session" }
                }
                repeat(input.count(MAX_OPERATIONS)) {
                    val id = input.str(); val payload = input.str(); val sessionId = input.str()
                    require(state.sessions.containsKey(sessionId)) { "cash operation references missing session" }
                    require(state.operations.put(id, Operation(payload, sessionId)) == null) { "duplicate cash operation" }
                }
                require(input.available() == 0) { "trailing cash session data" }
                validateState(state)
                return state
            }
        } catch (e: Exception) { throw IllegalArgumentException("invalid cash session file", e) }
    }

    private fun encode(s: State): ByteArray {
        validateState(s)
        val bytes = ByteArrayOutputStream().also { buffer -> DataOutputStream(buffer).use { out ->
            out.writeInt(MAGIC); out.writeInt(VERSION); out.writeInt(s.sessions.size)
            s.sessions.values.forEach { v ->
                out.str(v.id); out.str(v.drawerId); out.str(v.staffId); out.writeLong(v.openedAt); out.writeLong(v.openingFloatMinor)
                out.writeInt(v.status.ordinal); out.writeLong(v.expectedCashMinor)
                out.writeInt(v.movements.size); v.movements.forEach { m ->
                    out.str(m.operationId); out.writeInt(m.kind.ordinal); out.writeLong(m.amountMinor); out.str(m.reason); out.str(m.staffId); out.writeLong(m.recordedAt)
                }
                out.writeInt(v.sales.size); v.sales.forEach { sale ->
                    out.str(sale.saleId); out.str(sale.paymentId); out.writeLong(sale.amountMinor); out.writeLong(sale.tenderedMinor); out.writeLong(sale.changeMinor); out.writeLong(sale.recordedAt)
                    out.writeBoolean(sale.staffId != null); sale.staffId?.let { out.str(it) }
                }
                out.writeBoolean(v.closeout != null); v.closeout?.let { c ->
                    out.writeLong(c.countedMinor); out.writeLong(c.expectedMinor); out.writeLong(c.discrepancyMinor); out.str(c.staffId); out.writeLong(c.closedAt)
                }
            }
            out.writeInt(s.operations.size); s.operations.forEach { (id, op) -> out.str(id); out.str(op.payload); out.str(op.sessionId) }
        } }.toByteArray()
        require(bytes.size <= MAX_FILE_BYTES) { "cash session ledger exceeds storage limit" }
        return bytes
    }

    private fun validateState(s: State) {
        require(s.sessions.size <= MAX_SESSIONS && s.operations.size <= MAX_OPERATIONS) { "cash session ledger limit exceeded" }
        require(s.sessions.values.groupBy { it.drawerId }.values.all { group -> group.count { it.status == CashSessionStatus.OPEN } <= 1 }) {
            "multiple open sessions for drawer"
        }
        val paymentIds = mutableSetOf<String>()
        s.sessions.values.forEach { session ->
            validId(session.id, "session id"); validId(session.drawerId, "drawer id"); validId(session.staffId, "staff id")
            require(session.expectedCashMinor >= 0 && session.openingFloatMinor >= 0)
            var derived = session.openingFloatMinor
            session.movements.forEach { movement ->
                validId(movement.operationId, "operation id"); validText(movement.reason, "movement reason"); validId(movement.staffId, "staff id")
                require(movement.amountMinor > 0 && movement.recordedAt >= 0)
                val amount = if (movement.kind == CashMovementKind.CASH_IN) movement.amountMinor
                    else Math.negateExact(movement.amountMinor)
                derived = Math.addExact(derived, amount)
            }
            session.sales.forEach { sale ->
                validId(sale.saleId, "sale id"); validId(sale.paymentId, "payment id")
                sale.staffId?.let { validId(it, "staff id") }
                require(sale.amountMinor >= 0 && sale.tenderedMinor >= sale.amountMinor &&
                    sale.changeMinor == Math.subtractExact(sale.tenderedMinor, sale.amountMinor) && sale.recordedAt >= 0)
                derived = Math.addExact(derived, sale.amountMinor)
            }
            require(derived == session.expectedCashMinor) { "cash session expected balance is inconsistent" }
            require(session.sales.all { paymentIds.add(it.paymentId) }) { "duplicate cash payment link" }
            require((session.status == CashSessionStatus.CLOSED) == (session.closeout != null))
            session.closeout?.let { validId(it.staffId, "staff id") }
        }
        s.operations.keys.forEach { validId(it, "operation id") }
    }

    private inline fun <reified T : Enum<T>> enumAt(index: Int): T = enumValues<T>().getOrNull(index) ?: error("invalid enum value")
    private fun validId(value: String, label: String) { require(value.isNotBlank() && value.none { it.isISOControl() } && value.length <= MAX_TEXT_CHARS && value.toByteArray(Charsets.UTF_8).size <= MAX_TEXT_BYTES) { "invalid $label" } }
    private fun validText(value: String, label: String) { require(value.isNotBlank() && value.trim() == value && value.none { it.isISOControl() } && value.length <= MAX_TEXT_CHARS && value.toByteArray(Charsets.UTF_8).size <= MAX_TEXT_BYTES) { "invalid $label" } }
    private fun nonnegative(value: Long, label: String) { require(value >= 0) { "invalid $label" } }
    private fun <T> immutable(values: Collection<T>): List<T> = Collections.unmodifiableList(ArrayList(values))

    /** Length-prefix UTF-8 fields before hashing so embedded delimiters cannot merge payloads. */
    private fun fingerprint(vararg fields: String): String {
        val canonical = ByteArrayOutputStream().also { buffer -> DataOutputStream(buffer).use { out ->
            out.writeInt(fields.size)
            fields.forEach { field ->
                val bytes = field.toByteArray(Charsets.UTF_8)
                out.writeInt(bytes.size); out.write(bytes)
            }
        } }.toByteArray()
        return MessageDigest.getInstance("SHA-256").digest(canonical).joinToString("") {
            (it.toInt() and 0xff).toString(16).padStart(2, '0')
        }
    }

    private fun DataOutputStream.str(v: String) { val b = v.toByteArray(Charsets.UTF_8); require(b.size <= MAX_TEXT_BYTES); writeInt(b.size); write(b) }
    private fun DataInputStream.str(): String {
        val size = readInt(); require(size in 0..MAX_TEXT_BYTES)
        val bytes = ByteArray(size); readFully(bytes)
        return Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(bytes)).toString()
    }
    private fun DataInputStream.count(max: Int): Int = readInt().also { require(it in 0..max) }

    private companion object {
        const val MAGIC = 0x4f424353 // OBCS
        const val VERSION = 3
        const val MAX_FILE_BYTES = 2 * 1024 * 1024
        const val MAX_TEXT_BYTES = 512
        const val MAX_TEXT_CHARS = 256
        const val MAX_SESSIONS = 500
        const val MAX_MOVEMENTS = 1000
        const val MAX_SALES_PER_SESSION = 2000
        const val MAX_OPERATIONS = 10000
    }
}
