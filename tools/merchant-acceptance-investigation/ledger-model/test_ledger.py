import json
import tempfile
import unittest
from pathlib import Path

from ledger import Ledger


class Card:
    def __init__(self, result='paid', fail=False): self.result, self.fail, self.calls = result, fail, []
    def charge(self, **kw):
        self.calls.append(kw)
        if self.fail: raise TimeoutError('response lost')
        return self.result


class BTC:
    def __init__(self): self.calls = []
    def create_invoice(self, **kw): self.calls.append(kw); return 'invoice-test'


class SquareInventory:
    def __init__(self, fail_after_effect_once=False):
        self.calls = []; self.effects = {}; self.fail_after_effect_once = fail_after_effect_once
    def adjust_inventory(self, *, local_sale_reference, adjustment_key, lines):
        self.calls.append({'local_sale_reference':local_sale_reference,
                           'adjustment_key':adjustment_key, 'lines':lines})
        # Idempotent external receiver: apply once by stable outbox key, then simulate
        # a lost response after the effect on its first invocation.
        self.effects.setdefault(adjustment_key, lines)
        if self.fail_after_effect_once:
            self.fail_after_effect_once = False
            raise TimeoutError('effect committed; response lost')


class LedgerInvariants(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'ledger.sqlite'
        self.ledger = Ledger(self.path)

    def tearDown(self):
        self.ledger.close()
        self.tmp.cleanup()

    def sale(self):
        return self.ledger.create_sale(1250, [{'sku':'tea','quantity':2}])

    def test_btc_sale_never_calls_card_or_square_payment_and_local_acceptance_succeeds(self):
        sid = self.sale(); btc = BTC()
        attempt = self.ledger.create_btc_invoice(sid, btc)
        self.ledger.payment_callback(attempt, 'chain-event-1', 'paid', 'invoice-7')
        self.ledger.drain_inventory()
        self.assertEqual(self.ledger.sale(sid).status, 'accepted')
        self.assertEqual(self.ledger.inventory_quantity('tea'), -2)
        traces = self.ledger.commands()
        self.assertEqual([c['route'] for c in traces], ['bitcoin.create_invoice'])
        self.assertEqual(traces[0]['metadata']['attempt_id'], attempt)
        self.assertFalse(any('square' in c['route'] for c in traces))
        self.assertEqual(len(btc.calls), 1)

    def test_card_has_local_reference_and_stable_attempt_without_square_order_link(self):
        sid = self.sale(); card = Card()
        attempt = self.ledger.take_card(sid, card)
        trace = self.ledger.commands()[0]
        self.assertEqual(trace['route'], 'card.charge')
        self.assertEqual(trace['metadata']['local_sale_reference'], sid)
        self.assertEqual(trace['metadata']['attempt_id'], attempt)
        self.assertNotIn('square_order_id', trace['metadata'])
        self.assertNotIn('custom_square_order_id', trace['metadata'])
        self.assertEqual(card.calls[0]['attempt_id'], attempt)

    def test_ambiguous_card_blocks_fallback_until_resolution(self):
        sid = self.sale(); card = Card(fail=True)
        attempt = self.ledger.take_card(sid, card)
        self.assertEqual(self.ledger.attempts(sid)[0]['status'], 'ambiguous')
        with self.assertRaisesRegex(ValueError, 'resolved'):
            self.ledger.create_btc_invoice(sid, BTC())
        # An authoritative decline resolves the uncertainty; only then may fallback start.
        self.ledger.payment_callback(attempt, 'resolve-decline', 'declined')
        btc = BTC(); next_attempt = self.ledger.create_btc_invoice(sid, btc)
        self.assertNotEqual(attempt, next_attempt)
        self.assertEqual(len(btc.calls), 1)

    def test_restart_and_duplicate_callback_apply_one_inventory_delta(self):
        sid = self.sale(); btc = BTC(); attempt = self.ledger.create_btc_invoice(sid, btc)
        self.ledger.close()
        self.ledger = Ledger(self.path)
        self.assertTrue(self.ledger.payment_callback(attempt, 'same-chain-event', 'paid', 'invoice-8'))
        self.assertFalse(self.ledger.payment_callback(attempt, 'same-chain-event', 'paid', 'invoice-8'))
        # Crash/reopen after accepted transaction commits, before the outbox is drained.
        self.ledger.close()
        self.ledger = Ledger(self.path)
        self.assertEqual(self.ledger.sale(sid).status, 'accepted')
        self.assertFalse(self.ledger.payment_callback(attempt, 'same-chain-event', 'paid', 'invoice-8'))
        self.ledger.drain_inventory()
        self.assertEqual(self.ledger.inventory_quantity('tea'), -2)
        self.assertEqual(self.ledger.sale(sid).status, 'accepted')

    def test_inventory_sync_is_separate_and_retryable_nonpayment_outbox(self):
        sid = self.sale(); attempt = self.ledger.create_btc_invoice(sid, BTC())
        self.ledger.payment_callback(attempt, 'accepted-with-square-stock', 'paid', square_inventory=True)
        square = SquareInventory(fail_after_effect_once=True)
        self.ledger.drain_inventory(square)
        self.assertEqual(self.ledger.db.execute("SELECT status FROM inventory_outbox WHERE order_id=? AND kind='square'", (sid,)).fetchone()['status'], 'pending')
        self.ledger.close(); self.ledger = Ledger(self.path)
        self.ledger.drain_inventory(square)
        self.assertEqual([c['route'] for c in self.ledger.commands()],
                         ['bitcoin.create_invoice', 'square.inventory.adjust', 'square.inventory.adjust'])
        self.assertEqual(len(square.calls), 2)
        self.assertEqual(len(square.effects), 1)
        self.assertEqual(square.calls[0]['adjustment_key'], sid)
        self.assertEqual(square.calls[0]['lines'], [{'sku':'tea','quantity':2}])
        self.assertEqual(self.ledger.inventory_quantity('tea'), -2)
        self.assertFalse(any(c['route'] in ('square.payment','square.order.create') for c in self.ledger.commands()))

    def test_btc_refund_is_per_tender_and_does_not_create_square_command(self):
        sid = self.sale(); attempt = self.ledger.create_btc_invoice(sid, BTC())
        self.ledger.payment_callback(attempt, 'paid-for-refund', 'paid')
        self.ledger.record_refund(attempt, 400, 'btc-refund-1')
        self.assertEqual(self.ledger.record_refund(attempt, 400, 'btc-refund-1'), 'btc-refund-1')
        with self.assertRaisesRegex(ValueError, 'cumulative'):
            self.ledger.record_refund(attempt, 900, 'btc-refund-2')
        traces = self.ledger.commands()
        self.assertEqual([c['route'] for c in traces], ['bitcoin.create_invoice'])
        count = self.ledger.db.execute('SELECT count(*) FROM refunds WHERE attempt_id=?', (attempt,)).fetchone()[0]
        self.assertEqual(count, 1)
        self.assertEqual(self.ledger.db.execute('SELECT status FROM refunds WHERE id=?', ('btc-refund-1',)).fetchone()['status'], 'obligation')

    def test_cancel_then_late_btc_payment_is_flagged_not_reaccepted_or_restocked(self):
        sid = self.sale(); attempt = self.ledger.create_btc_invoice(sid, BTC())
        self.ledger.cancel(sid)
        self.ledger.payment_callback(attempt, 'late-chain-payment', 'paid', 'invoice-late')
        self.ledger.drain_inventory()
        self.assertEqual(self.ledger.sale(sid).status, 'cancelled')
        self.assertTrue(self.ledger.sale(sid).needs_attention)
        a = self.ledger.attempts(sid)[0]
        self.assertEqual(a['status'], 'late_paid')
        self.assertEqual(a['refund_required'], 1)
        self.assertEqual(self.ledger.inventory_quantity('tea'), 0)

    def test_pending_or_ambiguous_card_cannot_be_cancelled_away(self):
        sid = self.sale(); attempt = self.ledger.take_card(sid, Card(fail=True))
        with self.assertRaisesRegex(ValueError, 'card charge'):
            self.ledger.cancel(sid)
        self.assertEqual(self.ledger.sale(sid).status, 'open')
        self.assertEqual(self.ledger.attempts(sid)[0]['status'], 'ambiguous')
        self.ledger.payment_callback(attempt, 'resolved-paid', 'paid')
        self.assertEqual(self.ledger.sale(sid).status, 'accepted')

    def test_duplicate_sku_lines_are_aggregated_before_inventory_apply(self):
        sid = self.ledger.create_sale(1250, [{'sku':'tea','quantity':1}, {'sku':'tea','quantity':3}])
        attempt = self.ledger.create_btc_invoice(sid, BTC())
        self.ledger.payment_callback(attempt, 'duplicate-lines-paid', 'paid')
        self.ledger.drain_inventory()
        self.assertEqual(self.ledger.inventory_quantity('tea'), -4)
        self.assertEqual(self.ledger.db.execute('SELECT count(*) FROM inventory_events WHERE order_id=? AND sku=?', (sid,'tea')).fetchone()[0], 1)

    def test_declined_attempt_late_pay_after_fallback_is_surplus_and_not_double_stock(self):
        sid = self.sale(); old = self.ledger.create_btc_invoice(sid, BTC())
        self.ledger.payment_callback(old, 'terminal-decline', 'declined')
        card = Card(); current = self.ledger.take_card(sid, card)
        self.ledger.payment_callback(old, 'late-old-invoice-pay', 'paid', 'old-invoice')
        self.assertEqual(self.ledger.attempts(sid)[0]['status'], 'late_paid')
        self.assertEqual(self.ledger.sale(sid).status, 'accepted')
        self.assertTrue(self.ledger.sale(sid).needs_attention)
        self.ledger.drain_inventory()
        self.ledger.drain_inventory()
        self.assertEqual(self.ledger.inventory_quantity('tea'), -2)
        self.assertEqual(self.ledger.attempts(sid)[1]['id'], current)


if __name__ == '__main__':
    unittest.main()
