import os
import sys

from odoo.tests import tagged
from odoo.addons.point_of_sale.tests import test_frontend

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                '..', 'tests_standalone'))
import fake_box  # noqa: E402


@tagged('post_install', '-at_install', 'pos_openbmt')
class TestOpenbmtTour(test_frontend.TestPointOfSaleHttpCommon):
    def test_sale_is_paid_through_the_box(self):
        box = fake_box.FakeBox()
        self.addCleanup(box.close)
        box.advance = True
        box.currency = self.main_pos_config.currency_id.name
        method = self.env['pos.payment.method'].create({
            'name': 'Bitcoin',
            'payment_method_type': 'terminal',
            'use_payment_terminal': 'openbmt',
            'journal_id': self.bank_journal.id,
            'outstanding_account_id': self.bank_payment_method.outstanding_account_id.id,
            'openbmt_box_address': box.address,
            'openbmt_till_key': fake_box.KEY,
            'openbmt_counter': fake_box.COUNTER,
        })
        self.main_pos_config.write({'payment_method_ids': [(4, method.id)]})
        self.env['product.product'].create({
            'name': 'Bitcoin Pen',
            'available_in_pos': True,
            'list_price': 10.0,
            'taxes_id': False,
        })
        self.main_pos_config.with_user(self.pos_user).open_ui()
        # Needed because the tests run before the module is marked as
        # installed, and the page only loads templates of installed modules.
        self.env['ir.module.module'].search(
            [('name', '=', 'point_of_sale')], limit=1).state = 'installed'
        self.start_tour(
            '/pos/ui?config_id=%d' % self.main_pos_config.id,
            'OpenBMTPaymentTour', login='pos_user')
        order = self.env['pos.order'].search([], limit=1)
        self.assertEqual(order.state, 'paid')
        self.assertEqual(order.payment_ids.payment_method_id, method)
        self.assertEqual(len(order.payment_ids.transaction_id or ''), 32)
        self.assertIn('/till/v1/sales', [p for _, p, _ in box.seen])
