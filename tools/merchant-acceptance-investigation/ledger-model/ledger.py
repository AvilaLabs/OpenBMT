"""Small local-ledger routing prototype. No network calls or real provider SDKs."""
from __future__ import annotations

import json
import sqlite3
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol


def new_id() -> str:
    return str(uuid.uuid4())


class CardProvider(Protocol):
    def charge(self, *, local_sale_reference: str, attempt_id: str,
               amount_cents: int) -> str: ...  # paid | declined | ambiguous


class BitcoinReceiver(Protocol):
    def create_invoice(self, *, local_sale_reference: str, attempt_id: str,
                       amount_cents: int) -> str: ...


class SquareInventory(Protocol):
    def adjust_inventory(self, *, local_sale_reference: str,
                         adjustment_key: str, lines: list[dict]) -> None: ...


@dataclass(frozen=True)
class Sale:
    id: str
    amount_cents: int
    status: str
    needs_attention: bool


class Ledger:
    def __init__(self, path: str | Path):
        self.db = sqlite3.connect(str(path), isolation_level=None)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA foreign_keys=ON")
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.executescript("""
        CREATE TABLE IF NOT EXISTS local_orders(
          id TEXT PRIMARY KEY, amount_cents INTEGER NOT NULL CHECK(amount_cents>0),
          lines_json TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'open',
          needs_attention INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS tender_attempts(
          id TEXT PRIMARY KEY, order_id TEXT NOT NULL REFERENCES local_orders(id),
          tender TEXT NOT NULL CHECK(tender IN ('card','btc')),
          amount_cents INTEGER NOT NULL, status TEXT NOT NULL,
          provider_ref TEXT, refund_required INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS commands(
          seq INTEGER PRIMARY KEY AUTOINCREMENT, route TEXT NOT NULL,
          metadata_json TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS callback_events(
          attempt_id TEXT NOT NULL REFERENCES tender_attempts(id),
          event_id TEXT NOT NULL, PRIMARY KEY(attempt_id,event_id));
        CREATE TABLE IF NOT EXISTS inventory(
          sku TEXT PRIMARY KEY, quantity INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS inventory_outbox(
          id INTEGER PRIMARY KEY AUTOINCREMENT, order_id TEXT NOT NULL REFERENCES local_orders(id),
          kind TEXT NOT NULL CHECK(kind IN ('local','square')), status TEXT NOT NULL DEFAULT 'pending',
          UNIQUE(order_id,kind));
        CREATE TABLE IF NOT EXISTS inventory_events(
          order_id TEXT NOT NULL, sku TEXT NOT NULL, delta INTEGER NOT NULL,
          PRIMARY KEY(order_id,sku));
        CREATE TABLE IF NOT EXISTS refunds(
          id TEXT PRIMARY KEY, attempt_id TEXT NOT NULL REFERENCES tender_attempts(id),
          amount_cents INTEGER NOT NULL, status TEXT NOT NULL DEFAULT 'recorded');
        """)

    def close(self) -> None:
        self.db.close()

    def _tx(self):
        return _Transaction(self.db)

    def create_sale(self, amount_cents: int, lines: list[dict], sale_id: str | None = None) -> str:
        if amount_cents <= 0 or not lines or any(int(x['quantity']) <= 0 for x in lines):
            raise ValueError('positive sale amount and line quantities required')
        quantities: dict[str, int] = {}
        for line in lines:
            sku = str(line['sku'])
            if not sku:
                raise ValueError('SKU is required')
            quantities[sku] = quantities.get(sku, 0) + int(line['quantity'])
        normalized_lines = [{'sku': sku, 'quantity': qty} for sku, qty in sorted(quantities.items())]
        sid = sale_id or new_id()
        with self._tx():
            self.db.execute('INSERT INTO local_orders(id,amount_cents,lines_json) VALUES(?,?,?)',
                            (sid, amount_cents, json.dumps(normalized_lines, sort_keys=True)))
        return sid

    def sale(self, sale_id: str) -> Sale:
        r = self.db.execute('SELECT * FROM local_orders WHERE id=?', (sale_id,)).fetchone()
        return Sale(r['id'], r['amount_cents'], r['status'], bool(r['needs_attention']))

    def attempts(self, sale_id: str) -> list[dict]:
        return [dict(r) for r in self.db.execute(
            'SELECT * FROM tender_attempts WHERE order_id=? ORDER BY rowid', (sale_id,))]

    def _begin_attempt(self, sale_id: str, tender: str) -> tuple[str, int]:
        if tender not in ('card', 'btc'):
            raise ValueError('unknown tender')
        with self._tx():
            order = self.db.execute('SELECT * FROM local_orders WHERE id=?', (sale_id,)).fetchone()
            if order is None or order['status'] != 'open':
                raise ValueError('sale is not open')
            unresolved = self.db.execute("SELECT 1 FROM tender_attempts WHERE order_id=? AND status IN ('pending','ambiguous')", (sale_id,)).fetchone()
            if unresolved:
                raise ValueError('existing tender outcome must be resolved before fallback')
            if self.db.execute("SELECT 1 FROM tender_attempts WHERE order_id=? AND status IN ('accepted','late_paid')", (sale_id,)).fetchone():
                raise ValueError('sale already has a payment; split tender is not supported')
            attempt = new_id()
            self.db.execute('INSERT INTO tender_attempts(id,order_id,tender,amount_cents,status) VALUES(?,?,?,?,?)',
                            (attempt, sale_id, tender, order['amount_cents'], 'pending'))
            # Routing is visible in a durable trace before the external adapter is called.
            md = {'local_sale_reference': sale_id, 'attempt_id': attempt,
                  'amount_cents': order['amount_cents']}
            route = 'card.charge' if tender == 'card' else 'bitcoin.create_invoice'
            self.db.execute('INSERT INTO commands(route,metadata_json) VALUES(?,?)',
                            (route, json.dumps(md, sort_keys=True)))
            return attempt, order['amount_cents']

    def take_card(self, sale_id: str, provider: CardProvider) -> str:
        attempt, amount = self._begin_attempt(sale_id, 'card')
        try:
            result = provider.charge(local_sale_reference=sale_id, attempt_id=attempt,
                                     amount_cents=amount)
        except Exception:
            result = 'ambiguous'
        if result not in ('paid', 'declined', 'ambiguous'):
            result = 'ambiguous'
        if result != 'ambiguous':
            self.payment_callback(attempt, 'card-result-' + attempt, result,
                                  provider_ref='test-card-' + attempt)
        else:
            with self._tx():
                self.db.execute("UPDATE tender_attempts SET status='ambiguous' WHERE id=? AND status='pending'", (attempt,))
        return attempt

    def create_btc_invoice(self, sale_id: str, receiver: BitcoinReceiver) -> str:
        attempt, amount = self._begin_attempt(sale_id, 'btc')
        try:
            receiver.create_invoice(local_sale_reference=sale_id, attempt_id=attempt,
                                    amount_cents=amount)
        except Exception:
            with self._tx():
                self.db.execute("UPDATE tender_attempts SET status='ambiguous' WHERE id=? AND status='pending'", (attempt,))
        return attempt

    def payment_callback(self, attempt_id: str, event_id: str, outcome: str,
                         provider_ref: str | None = None, square_inventory: bool = False) -> bool:
        if outcome not in ('paid', 'declined'):
            raise ValueError('callback outcome must be paid or declined')
        with self._tx():
            a = self.db.execute('SELECT * FROM tender_attempts WHERE id=?', (attempt_id,)).fetchone()
            if a is None:
                raise ValueError('unknown attempt')
            cur = self.db.execute('INSERT OR IGNORE INTO callback_events VALUES(?,?)', (attempt_id, event_id))
            if cur.rowcount == 0:
                return False
            order = self.db.execute('SELECT * FROM local_orders WHERE id=?', (a['order_id'],)).fetchone()
            if outcome == 'declined':
                if a['status'] in ('pending', 'ambiguous'):
                    self.db.execute("UPDATE tender_attempts SET status='declined',provider_ref=? WHERE id=?", (provider_ref, attempt_id))
                return True
            # A paid callback after cancel/decline, or after another payment, is surplus.
            already_paid = self.db.execute("SELECT 1 FROM tender_attempts WHERE order_id=? AND status='accepted'", (a['order_id'],)).fetchone()
            late = order['status'] == 'cancelled' or a['status'] in ('cancelled','declined','late_paid') or (already_paid and a['status'] != 'accepted')
            if late:
                self.db.execute("UPDATE tender_attempts SET status='late_paid',provider_ref=?,refund_required=1 WHERE id=?", (provider_ref, attempt_id))
                self.db.execute('UPDATE local_orders SET needs_attention=1 WHERE id=?', (a['order_id'],))
                return True
            if a['status'] == 'accepted':
                return True
            self.db.execute("UPDATE tender_attempts SET status='accepted',provider_ref=? WHERE id=?", (provider_ref, attempt_id))
            self.db.execute("UPDATE local_orders SET status='accepted' WHERE id=?", (a['order_id'],))
            self.db.execute("INSERT OR IGNORE INTO inventory_outbox(order_id,kind) VALUES(?,'local')", (a['order_id'],))
            if square_inventory:
                self.db.execute("INSERT OR IGNORE INTO inventory_outbox(order_id,kind) VALUES(?,'square')", (a['order_id'],))
            return True

    def cancel(self, sale_id: str) -> None:
        with self._tx():
            order = self.db.execute('SELECT * FROM local_orders WHERE id=?', (sale_id,)).fetchone()
            if order is None or order['status'] != 'open':
                raise ValueError('sale is not open')
            attempts = self.db.execute("SELECT * FROM tender_attempts WHERE order_id=? AND status IN ('pending','ambiguous')", (sale_id,)).fetchall()
            if any(a['tender'] == 'card' for a in attempts):
                raise ValueError('ambiguous or pending card charge must be resolved before cancellation')
            # BTC invoices can still pay after local cancellation. Mark locally cancelled,
            # retain their attempt, and continue accepting callbacks for surplus detection.
            self.db.execute("UPDATE local_orders SET status='cancelled' WHERE id=?", (sale_id,))
            self.db.execute("UPDATE tender_attempts SET status='cancelled' WHERE order_id=? AND tender='btc' AND status IN ('pending','ambiguous')", (sale_id,))

    def record_refund(self, attempt_id: str, amount_cents: int, refund_id: str) -> str:
        if not refund_id or amount_cents <= 0:
            raise ValueError('stable refund ID and positive amount required')
        with self._tx():
            a = self.db.execute('SELECT * FROM tender_attempts WHERE id=?', (attempt_id,)).fetchone()
            if a is None or a['status'] not in ('accepted','late_paid'):
                raise ValueError('refund must reference a paid tender')
            prior = self.db.execute('SELECT * FROM refunds WHERE id=?', (refund_id,)).fetchone()
            if prior is not None:
                if prior['attempt_id'] != attempt_id or prior['amount_cents'] != amount_cents:
                    raise ValueError('refund ID already used with different details')
                return refund_id
            refunded = self.db.execute('SELECT COALESCE(SUM(amount_cents),0) AS total FROM refunds WHERE attempt_id=?', (attempt_id,)).fetchone()['total']
            if refunded + amount_cents > a['amount_cents']:
                raise ValueError('cumulative refunds exceed paid tender amount')
            refund_status = 'obligation' if a['tender'] == 'btc' else 'commanded'
            self.db.execute('INSERT INTO refunds(id,attempt_id,amount_cents,status) VALUES(?,?,?,?)',
                            (refund_id,attempt_id,amount_cents,refund_status))
            if a['tender'] == 'card':
                # A production adapter would consume this distinct refund command.
                self.db.execute('INSERT INTO commands(route,metadata_json) VALUES(?,?)',
                    ('card.refund', json.dumps({'attempt_id':attempt_id,'refund_id':refund_id,'amount_cents':amount_cents},sort_keys=True)))
            # BTC refunds are local obligations; they never route through Square.
        return refund_id

    def drain_inventory(self, square: SquareInventory | None = None) -> None:
        rows = self.db.execute("SELECT * FROM inventory_outbox WHERE status='pending' ORDER BY id").fetchall()
        for row in rows:
            order = self.db.execute('SELECT * FROM local_orders WHERE id=?', (row['order_id'],)).fetchone()
            lines = json.loads(order['lines_json'])
            if row['kind'] == 'local':
                with self._tx():
                    for line in lines:
                        sku, qty = line['sku'], int(line['quantity'])
                        self.db.execute('INSERT OR IGNORE INTO inventory(sku,quantity) VALUES(?,0)', (sku,))
                        self.db.execute('INSERT OR IGNORE INTO inventory_events VALUES(?,?,?)',
                                        (row['order_id'], sku, -qty))
                        if self.db.execute('SELECT changes()').fetchone()[0]:
                            self.db.execute('UPDATE inventory SET quantity=quantity-? WHERE sku=?', (qty,sku))
                    self.db.execute("UPDATE inventory_outbox SET status='done' WHERE id=?", (row['id'],))
            elif square is not None:
                metadata = {'local_sale_reference':row['order_id'], 'kind':'inventory_adjustment'}
                with self._tx():
                    self.db.execute('INSERT INTO commands(route,metadata_json) VALUES(?,?)',
                                    ('square.inventory.adjust', json.dumps(metadata,sort_keys=True)))
                try:
                    square.adjust_inventory(local_sale_reference=row['order_id'],
                                            adjustment_key=row['order_id'], lines=lines)
                except Exception:
                    continue  # retry is at-least-once; adapter must use sale ref as idempotency key
                with self._tx():
                    self.db.execute("UPDATE inventory_outbox SET status='done' WHERE id=?", (row['id'],))

    def inventory_quantity(self, sku: str) -> int:
        r = self.db.execute('SELECT quantity FROM inventory WHERE sku=?', (sku,)).fetchone()
        return 0 if r is None else r['quantity']

    def commands(self) -> list[dict]:
        return [{'route':r['route'], 'metadata':json.loads(r['metadata_json'])}
                for r in self.db.execute('SELECT route,metadata_json FROM commands ORDER BY seq')]


class _Transaction:
    def __init__(self, db): self.db = db
    def __enter__(self): self.db.execute('BEGIN IMMEDIATE'); return self
    def __exit__(self, typ, val, tb):
        self.db.execute('ROLLBACK' if typ else 'COMMIT')
