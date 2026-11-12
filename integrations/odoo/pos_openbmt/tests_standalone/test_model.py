import importlib.util
import logging
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, 'odoo_stub'))
sys.path.insert(0, HERE)

import fake_box  # noqa: E402

MODEL_PATH = os.path.join(HERE, '..', 'models', 'pos_payment_method.py')
spec = importlib.util.spec_from_file_location('pos_payment_method', MODEL_PATH)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
Method = mod.PosPaymentMethod


class Base(unittest.TestCase):
    def setUp(self):
        self.box = fake_box.FakeBox()
        self.addCleanup(self.box.close)
        self.method = self.make()
        self.logs = []

        class Grab(logging.Handler):
            def emit(handler, record):
                self.logs.append(record.getMessage())

        self.handler = Grab()
        logging.getLogger(mod.__name__).addHandler(self.handler)
        self.addCleanup(logging.getLogger(mod.__name__).removeHandler, self.handler)

    def make(self, **over):
        values = dict(openbmt_box_address=self.box.address,
                      openbmt_till_key=fake_box.KEY,
                      openbmt_counter=fake_box.COUNTER)
        values.update(over)
        return Method(**values)

    def start(self, request='req-1', amount='12.50', currency='EUR', method=None):
        return (method or self.method).openbmt_start(
            amount, currency, 'Order 00012-003-0004', request)

    def assert_clean(self, *results):
        blob = repr(results) + ' '.join(self.logs)
        self.assertNotIn(fake_box.KEY, blob)
        self.assertNotIn('Bearer', blob)


class Selection(unittest.TestCase):
    def test_selection_and_fields(self):
        self.assertEqual(Method()._get_payment_terminal_selection()[-1],
                         ('openbmt', 'OpenBMT'))
        self.assertEqual(Method.openbmt_till_key.groups,
                         'point_of_sale.group_pos_manager')
        self.assertEqual(Method.openbmt_box_address.string, 'Box address')
        self.assertEqual(Method.openbmt_till_key.string, 'Till key')
        self.assertEqual(Method.openbmt_counter.string, 'Counter code')

    def test_counter_constraint(self):
        for good in ('1a2b3c4d', '', None):
            Method(openbmt_counter=good)._check_openbmt_counter()
        for bad in ('1A2B3C4D', '1a2b3c4', '1a2b3c4d5', 'zzzzzzzz', '1a2b 3c4'):
            with self.assertRaises(mod.ValidationError):
                Method(openbmt_counter=bad)._check_openbmt_counter()


class States(Base):
    def test_waiting_opened_paid(self):
        r = self.start()
        self.assertEqual(r['state'], 'waiting')
        self.assertIsNone(r['words'])
        sale = r['sale']
        self.assertEqual(self.method.openbmt_status(sale)['state'], 'waiting')
        self.box.set_state(sale, 'opened')
        self.assertEqual(self.method.openbmt_status(sale)['state'], 'opened')
        self.box.set_state(sale, 'paid')
        paid = self.method.openbmt_status(sale)
        self.assertEqual(paid['state'], 'paid')
        self.assertEqual(paid['words']['what'], 'Paid in bitcoin.')
        self.assert_clean(r, paid)

    def test_end_states_carry_words(self):
        for state in ('unpaid', 'expired', 'cancelled'):
            sale = self.start(request='r-' + state)['sale']
            self.box.set_state(sale, state)
            r = self.method.openbmt_status(sale)
            self.assertEqual(r['state'], state)
            self.assertEqual(set(r['words']), {'heading', 'what', 'next'})
            self.assertTrue(r['words']['heading'])

    def test_same_request_same_sale(self):
        a = self.start(request='same')
        b = self.start(request='same')
        c = self.start(request='other')
        self.assertEqual(a['sale'], b['sale'])
        self.assertNotEqual(a['sale'], c['sale'])

    def test_cancel(self):
        sale = self.start()['sale']
        r = self.method.openbmt_cancel(sale)
        self.assertEqual(r['state'], 'cancelled')
        self.assertEqual(self.method.openbmt_status(sale)['state'], 'cancelled')

    def test_cancel_keeps_paid(self):
        sale = self.start()['sale']
        self.box.set_state(sale, 'paid')
        self.assertEqual(self.method.openbmt_cancel(sale)['state'], 'paid')

    def test_bearer_header_sent(self):
        self.start()
        self.assertEqual(self.box.seen[-1],
                         ('POST', '/till/v1/sales', 'Bearer ' + fake_box.KEY))

    def test_trailing_slash_address(self):
        m = self.make(openbmt_box_address=self.box.address + '/')
        self.assertEqual(self.start(method=m)['state'], 'waiting')


class Refusals(Base):
    def refused(self, r, key):
        self.assertEqual(r['error'], key)
        self.assertEqual(set(r['words']), {'heading', 'what', 'next'})
        self.assert_clean(r)

    def test_wrong_key(self):
        m = self.make(openbmt_till_key='cd' * 32)
        self.refused(self.start(method=m), 'tillKeyWrong')
        self.assertNotIn('cd' * 32, repr(self.logs))

    def test_not_paired(self):
        self.box.paired = False
        self.refused(self.start(), 'tillNotPaired')

    def test_unknown_counter(self):
        m = self.make(openbmt_counter='00000000')
        self.refused(self.start(method=m), 'tillUnknownCounter')

    def test_wrong_money(self):
        self.refused(self.start(currency='USD'), 'tillWrongMoney')

    def test_bad_amount(self):
        self.refused(self.start(amount='0.00'), 'tillBadAmount')
        self.refused(self.start(amount='1.234'), 'tillBadAmount')

    def test_busy(self):
        for i in range(fake_box.MAX_WAITING):
            self.assertEqual(self.start(request='r%d' % i)['state'], 'waiting')
        self.refused(self.start(request='one-more'), 'tillBusy')

    def test_lost_sale(self):
        self.refused(self.method.openbmt_status('0' * 32), 'tillLost')
        self.refused(self.method.openbmt_cancel('0' * 32), 'tillLost')

    def test_bad_inputs_never_leave_the_server(self):
        before = len(self.box.seen)
        for r in (self.start(amount='-1'), self.start(amount='1e3'),
                  self.start(currency='eur'), self.start(request='a b'),
                  self.start(request=''), self.start(request='x' * 65),
                  self.method.openbmt_status('../..'),
                  self.method.openbmt_cancel('A' * 32),
                  self.method.openbmt_status(None)):
            self.assertEqual(r['error'], mod.ERR_SETUP)
        self.assertEqual(len(self.box.seen), before)

    def test_setup_missing(self):
        for over in ({'openbmt_till_key': ''}, {'openbmt_counter': 'bad'},
                     {'openbmt_box_address': ''},
                     {'openbmt_box_address': 'ftp://127.0.0.1:1'},
                     {'openbmt_box_address': 'file:///etc/passwd'},
                     {'openbmt_box_address': 'http://user:pw@127.0.0.1:1'},
                     {'openbmt_box_address': 'http://127.0.0.1:1/x?y=1'},
                     {'openbmt_box_address': 'http://127.0.0.1:99999'},
                     {'openbmt_box_address': 'http://'}):
            r = self.start(method=self.make(**over))
            self.assertEqual(r, {'error': mod.ERR_SETUP, 'words': None}, over)
        self.assertEqual(self.box.seen, [])

    def test_https_scheme_accepted_by_check(self):
        m = self.make(openbmt_box_address='https://box.example:8091')
        self.assertEqual(m._openbmt_base(), 'https://box.example:8091')


class Transport(Base):
    def test_connection_refused(self):
        m = self.make(openbmt_box_address='http://127.0.0.1:1')
        r = self.start(method=m)
        self.assertEqual(r, {'error': mod.ERR_TRANSPORT, 'words': None})
        self.assert_clean(r)

    def test_timeout(self):
        old = mod.TIMEOUT
        mod.TIMEOUT = 0.3
        self.addCleanup(setattr, mod, 'TIMEOUT', old)
        self.box.mode = 'slow'
        self.box.delay = 1.5
        r = self.start()
        self.assertEqual(r, {'error': mod.ERR_TRANSPORT, 'words': None})
        self.assert_clean(r)

    def test_redirect_refused(self):
        self.box.mode = 'redirect'
        r = self.start()
        self.assertEqual(r, {'error': mod.ERR_ANSWER, 'words': None})
        self.assertEqual([p for _, p, _ in self.box.seen], ['/till/v1/sales'])

    def test_garbage_refused(self):
        self.box.mode = 'garbage'
        self.assertEqual(self.start()['error'], mod.ERR_ANSWER)

    def test_shapes_refused(self):
        sale = 'a' * 32
        bad = [
            [], 'x', {}, {'sale': sale}, {'state': 'waiting'},
            {'sale': 'A' * 32, 'state': 'waiting'},
            {'sale': 'a' * 31, 'state': 'waiting'},
            {'sale': sale, 'state': 'refunded'},
            {'sale': sale, 'state': 7},
            {'sale': sale, 'state': 'paid'},
            {'sale': sale, 'state': 'unpaid', 'words': 'text'},
            {'sale': sale, 'state': 'unpaid', 'words': {'heading': 1, 'what': 'x', 'next': 'y'}},
            {'sale': sale, 'state': 'unpaid', 'words': {'heading': 'h', 'what': '', 'next': 'y'}},
            {'sale': sale, 'state': 'waiting', 'words': {'heading': 'h', 'what': 'w', 'next': 'n'}},
            {'sale': 'b' * 32, 'state': 'waiting'},
        ]
        self.box.mode = 'shape'
        for shape in bad:
            self.box.shape = shape
            self.assertEqual(self.method.openbmt_status(sale),
                             {'error': mod.ERR_ANSWER, 'words': None}, shape)

    def test_oversize_answer_refused(self):
        self.box.mode = 'shape'
        self.box.shape = {'sale': 'a' * 32, 'state': 'waiting', 'pad': 'x' * 40000}
        self.assertEqual(self.method.openbmt_status('a' * 32)['error'],
                         mod.ERR_ANSWER)

    def test_good_shape_accepted(self):
        self.box.mode = 'shape'
        self.box.shape = {'sale': 'a' * 32, 'state': 'opened'}
        self.assertEqual(self.method.openbmt_status('a' * 32),
                         {'state': 'opened', 'sale': 'a' * 32, 'words': None})

    def test_error_body_shapes_refused(self):
        parse = self.method._openbmt_parse
        w = b'{"heading":"h","what":"w","next":"n"}'
        for status, raw in ((401, b'{}'), (404, b'{}'), (500, b'{"error":"tillX","words":' + w + b'}'),
                            (401, b'{"error":"nope","words":' + w + b'}'),
                            (401, b'{"error":"till Key","words":' + w + b'}'),
                            (401, b'{"error":"tillKeyWrong"}'), (401, b'[1]'),
                            (302, b'{}')):
            self.assertEqual(parse(status, raw, None),
                             {'error': mod.ERR_ANSWER, 'words': None}, (status, raw))

    def test_failure_log_has_class_name_only(self):
        m = self.make(openbmt_box_address='http://127.0.0.1:1')
        self.start(method=m)
        self.assertTrue(self.logs)
        self.assertTrue(all('ConnectionError' in line for line in self.logs))
        self.assert_clean()


class Files(unittest.TestCase):
    def test_view_is_well_formed_and_names_fields(self):
        import xml.etree.ElementTree as ET
        root = ET.parse(os.path.join(HERE, '..', 'views',
                                     'pos_payment_method_views.xml')).getroot()
        names = {f.get('name') for f in root.iter('field')}
        for n in ('openbmt_box_address', 'openbmt_till_key', 'openbmt_counter'):
            self.assertIn(n, names)

    def test_manifest(self):
        with open(os.path.join(HERE, '..', '__manifest__.py')) as f:
            m = eval(compile(f.read(), 'm', 'eval'))
        self.assertEqual(m['depends'], ['point_of_sale'])
        self.assertEqual(m['license'], 'Other OSI approved licence')
        self.assertIn('point_of_sale._assets_pos', m['assets'])
        self.assertTrue(os.path.exists(os.path.join(HERE, '..', 'LICENSE')))


if __name__ == '__main__':
    unittest.main()
