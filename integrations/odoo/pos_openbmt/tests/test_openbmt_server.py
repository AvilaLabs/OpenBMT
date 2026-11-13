import os
import sys

from odoo.tests import tagged
from odoo.tests.common import TransactionCase

# The fake Box used by the standalone tests is the same file.
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                '..', 'tests_standalone'))
import fake_box  # noqa: E402


@tagged('post_install', '-at_install', 'pos_openbmt')
class TestOpenbmtServer(TransactionCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.box = fake_box.FakeBox()
        cls.addClassCleanup(cls.box.close)
        cls.method = cls.make()

    @classmethod
    def make(cls, **over):
        values = {
            'name': 'Bitcoin',
            'payment_method_type': 'terminal',
            'use_payment_terminal': 'openbmt',
            'split_transactions': True,
            'openbmt_box_address': cls.box.address,
            'openbmt_till_key': fake_box.KEY,
            'openbmt_counter': fake_box.COUNTER,
        }
        values.update(over)
        return cls.env['pos.payment.method'].create(values)

    def start(self, request='req-1', amount='12.50', currency='EUR'):
        return self.method.openbmt_start(
            amount, currency, 'Order 00012-003-0004', request)

    def test_terminal_is_offered(self):
        selection = self.env['pos.payment.method']._get_payment_terminal_selection()
        self.assertIn(('openbmt', 'OpenBMT'), selection)
        self.assertEqual(self.method.use_payment_terminal, 'openbmt')

    def test_states(self):
        r = self.start()
        self.assertEqual(r['state'], 'waiting')
        sale = r['sale']
        self.box.set_state(sale, 'opened')
        self.assertEqual(self.method.openbmt_status(sale)['state'], 'opened')
        self.box.set_state(sale, 'paid')
        paid = self.method.openbmt_status(sale)
        self.assertEqual(paid['state'], 'paid')
        self.assertEqual(paid['words']['what'], 'Paid in bitcoin.')
        for state in ('unpaid', 'expired', 'cancelled'):
            other = self.start(request='r-' + state)['sale']
            self.box.set_state(other, state)
            self.assertEqual(self.method.openbmt_status(other)['state'], state)
        self.assertEqual(self.start()['sale'], sale)

    def test_cancel(self):
        sale = self.start(request='c1')['sale']
        self.assertEqual(self.method.openbmt_cancel(sale)['state'], 'cancelled')
        paid = self.start(request='c2')['sale']
        self.box.set_state(paid, 'paid')
        self.assertEqual(self.method.openbmt_cancel(paid)['state'], 'paid')

    def test_refusals(self):
        self.assertEqual(self.start(currency='USD')['error'], 'tillWrongMoney')
        self.assertEqual(self.start(amount='0.00')['error'], 'tillBadAmount')
        wrong = self.make(openbmt_till_key='cd' * 32)
        r = wrong.openbmt_start('1.00', 'EUR', 'x', 'k1')
        # The Box signs its refusal under the reply key of its own key, so
        # an answer to another key does not check.
        self.assertEqual(r['error'], 'tillNotSigned')
        self.assertNotIn('cd' * 32, repr(r))
        other = self.make(openbmt_counter='00000000')
        self.assertEqual(other.openbmt_start('1.00', 'EUR', 'x', 'k2')['error'],
                         'tillUnknownCounter')
        self.assertEqual(self.method.openbmt_status('0' * 32)['error'], 'tillLost')
        self.box.paired = False
        self.addCleanup(setattr, self.box, 'paired', True)
        self.assertEqual(self.start(request='k3')['error'], 'tillNotPaired')

    def test_local_errors(self):
        r = self.make(openbmt_box_address='http://127.0.0.1:1').openbmt_start(
            '1.00', 'EUR', 'x', 'k4')
        self.assertEqual(r['error'], 'tillTransport')
        self.assertEqual(self.method.openbmt_start('1e3', 'EUR', 'x', 'k5')['error'],
                         'tillNotSet')
        self.assertEqual(self.method.openbmt_status('../..')['error'], 'tillNotSet')
        self.box.mode = 'garbage'
        self.addCleanup(setattr, self.box, 'mode', 'normal')
        self.assertEqual(self.start(request='k6')['error'], 'tillBadAnswer')

    def test_counter_code_checked(self):
        from odoo.exceptions import ValidationError
        with self.assertRaises(ValidationError):
            self.method.write({'openbmt_counter': '1A2B3C4D'})

    def test_till_key_not_sent_to_the_browser(self):
        loaded = self.env['pos.payment.method']._load_pos_data_fields(0)
        for name in ('openbmt_till_key', 'openbmt_box_address', 'openbmt_counter'):
            self.assertNotIn(name, loaded)

    def test_till_key_hidden_from_cashiers(self):
        from odoo.exceptions import AccessError
        cashier = self.env['res.users'].create({
            'name': 'Cashier',
            'login': 'openbmt_cashier',
            'groups_id': [(6, 0, [self.env.ref('point_of_sale.group_pos_user').id])],
        })
        as_cashier = self.method.with_user(cashier)
        with self.assertRaises(AccessError):
            as_cashier.read(['openbmt_till_key'])
        self.assertEqual(as_cashier.openbmt_start(
            '1.00', 'EUR', 'x', 'k7')['state'], 'waiting')
