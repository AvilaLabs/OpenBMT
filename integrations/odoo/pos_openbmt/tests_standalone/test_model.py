import importlib.util
import json
import logging
import os
import sys
import time
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

    def status(self, sale, request='req-1', amount='12.50', currency='EUR',
               method=None):
        return (method or self.method).openbmt_status(
            sale, amount, currency, 'Order 00012-003-0004', request)

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
        self.assertEqual(self.status(sale)['state'], 'waiting')
        self.box.set_state(sale, 'opened')
        self.assertEqual(self.status(sale)['state'], 'opened')
        self.box.set_state(sale, 'paid')
        paid = self.status(sale)
        self.assertEqual(paid['state'], 'paid')
        self.assertEqual(paid['words']['what'], 'Paid in bitcoin.')
        self.assert_clean(r, paid)

    def test_end_states_carry_words(self):
        for state in ('unpaid', 'expired', 'cancelled'):
            sale = self.start(request='r-' + state)['sale']
            self.box.set_state(sale, state)
            r = self.status(sale, request='r-' + state)
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
        self.assertEqual(self.status(sale)['state'], 'cancelled')

    def test_cancel_keeps_paid(self):
        sale = self.start()['sale']
        self.box.set_state(sale, 'paid')
        self.assertEqual(self.method.openbmt_cancel(sale)['state'], 'paid')

    def test_bearer_header_sent(self):
        self.start()
        self.assertEqual(self.box.seen[-1],
                         ('POST', '/till/v1/sales',
                          'Bearer ' + mod.till_token(fake_box.KEY)))

    def test_trailing_slash_address(self):
        m = self.make(openbmt_box_address=self.box.address + '/')
        self.assertEqual(self.start(method=m)['state'], 'waiting')


class Refusals(Base):
    def refused(self, r, key):
        self.assertEqual(r['error'], key)
        self.assertEqual(set(r['words']), {'heading', 'what', 'next'})
        self.assert_clean(r)

    def test_wrong_key(self):
        # The Box signs its refusal under the reply key of its own key, which
        # the module cannot check with another key, so the answer is not
        # taken.
        m = self.make(openbmt_till_key='cd' * 32)
        self.refused(self.start(method=m), mod.ERR_UNSIGNED)
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
        self.refused(self.status('0' * 32), 'tillLost')
        self.refused(self.method.openbmt_cancel('0' * 32), 'tillLost')

    def test_bad_inputs_never_leave_the_server(self):
        before = len(self.box.seen)
        for r in (self.start(amount='-1'), self.start(amount='1e3'),
                  self.start(currency='eur'), self.start(request='a b'),
                  self.start(request=''), self.start(request='x' * 65),
                  self.status('../..'),
                  self.method.openbmt_cancel('A' * 32),
                  self.status(None),
                  self.status('a' * 32, amount='1e3'),
                  self.status('a' * 32, request='a b')):
            self.assertEqual(r['error'], mod.ERR_SETUP)
        self.assertEqual(len(self.box.seen), before)

    def test_setup_missing(self):
        for over in ({'openbmt_till_key': ''}, {'openbmt_counter': 'bad'},
                     {'openbmt_till_key': 'ab' * 31},
                     {'openbmt_till_key': 'AB' * 32},
                     {'openbmt_till_key': 'zz' * 32},
                     {'openbmt_box_address': ''},
                     {'openbmt_box_address': 'ftp://127.0.0.1:1'},
                     {'openbmt_box_address': 'file:///etc/passwd'},
                     {'openbmt_box_address': 'http://user:pw@127.0.0.1:1'},
                     {'openbmt_box_address': 'http://127.0.0.1:1/x?y=1'},
                     {'openbmt_box_address': 'http://127.0.0.1:99999'},
                     {'openbmt_box_address': 'http://'}):
            r = self.start(method=self.make(**over))
            self.assertEqual(r, {'error': mod.ERR_SETUP, 'words': mod.LOCAL_WORDS[mod.ERR_SETUP]}, over)
        self.assertEqual(self.box.seen, [])

    def test_https_scheme_accepted_by_check(self):
        m = self.make(openbmt_box_address='https://box.example:8091')
        self.assertEqual(m._openbmt_base(), 'https://box.example:8091')


class Transport(Base):
    def test_connection_refused(self):
        m = self.make(openbmt_box_address='http://127.0.0.1:1')
        r = self.start(method=m)
        self.assertEqual(r, {'error': mod.ERR_TRANSPORT, 'words': mod.LOCAL_WORDS[mod.ERR_TRANSPORT]})
        self.assert_clean(r)

    def test_timeout(self):
        old = mod.TIMEOUT
        mod.TIMEOUT = 0.3
        self.addCleanup(setattr, mod, 'TIMEOUT', old)
        self.box.mode = 'slow'
        self.box.delay = 1.5
        r = self.start()
        self.assertEqual(r, {'error': mod.ERR_TRANSPORT, 'words': mod.LOCAL_WORDS[mod.ERR_TRANSPORT]})
        self.assert_clean(r)

    def test_a_box_that_drips_bytes_is_cut_off_at_the_deadline(self):
        # One byte a second: every read is well inside TIMEOUT, so only the
        # deadline of the whole call stops it.
        old = mod.DEADLINE
        mod.DEADLINE = 2
        self.addCleanup(setattr, mod, 'DEADLINE', old)
        self.box.mode = 'drip'
        self.box.gap = 1.0
        began = time.monotonic()
        r = self.start()
        took = time.monotonic() - began
        self.assertEqual(r, {'error': mod.ERR_TRANSPORT, 'words': mod.LOCAL_WORDS[mod.ERR_TRANSPORT]})
        self.assertGreaterEqual(took, 1.5)
        self.assertLess(took, 5)
        self.assert_clean(r)

    def test_redirect_refused(self):
        self.box.mode = 'redirect'
        r = self.start()
        self.assertEqual(r, {'error': mod.ERR_ANSWER, 'words': mod.LOCAL_WORDS[mod.ERR_ANSWER]})
        self.assertEqual([p for _, p, _ in self.box.seen], ['/till/v1/sales'])

    def test_garbage_refused(self):
        self.box.mode = 'garbage'
        self.assertEqual(self.start()['error'], mod.ERR_ANSWER)

    def test_shapes_refused(self):
        sale = 'a' * 32
        # The answer names what was asked for, so that only the shape differs.
        echo = {'amount': '12.50', 'currency': 'EUR', 'request': 'req-1',
                'reference': 'Order 00012-003-0004'}
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
            if isinstance(shape, dict):
                shape = dict(shape, **echo)
            self.box.shape = shape
            self.assertEqual(self.status(sale),
                             {'error': mod.ERR_ANSWER, 'words': mod.LOCAL_WORDS[mod.ERR_ANSWER]}, shape)

    def test_oversize_answer_refused(self):
        self.box.mode = 'shape'
        self.box.shape = {'sale': 'a' * 32, 'state': 'waiting', 'pad': 'x' * 40000}
        self.assertEqual(self.status('a' * 32)['error'],
                         mod.ERR_ANSWER)

    def test_good_shape_accepted(self):
        self.box.mode = 'shape'
        self.box.shape = {'sale': 'a' * 32, 'state': 'opened',
                          'amount': '12.50', 'currency': 'EUR',
                          'request': 'req-1',
                          'reference': 'Order 00012-003-0004'}
        self.assertEqual(self.status('a' * 32),
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
                             {'error': mod.ERR_ANSWER, 'words': mod.LOCAL_WORDS[mod.ERR_ANSWER]}, (status, raw))

    def test_failure_log_has_class_name_only(self):
        m = self.make(openbmt_box_address='http://127.0.0.1:1')
        self.start(method=m)
        self.assertTrue(self.logs)
        self.assertTrue(all('ConnectionError' in line for line in self.logs))
        self.assert_clean()


def vectors():
    with open(os.path.join(HERE, 'till_vectors.json'), encoding='utf-8') as f:
        return json.load(f)


class Vectors(unittest.TestCase):
    """What the Box's own code makes, written by its test (BX-80)."""

    def test_token_and_reply_key_of_the_key(self):
        v = vectors()
        self.assertEqual(mod.till_token(v['key']), v['token'])
        self.assertEqual(mod.till_reply_key(v['key']).hex(), v['reply_key'])
        # The stored file: version, hash of the token, reply key.
        stored = bytes.fromhex(v['key_file'])
        self.assertEqual(stored[0], 2)
        self.assertEqual(
            stored[1:33],
            mod.tagged_hash('OpenBMT/till-key/v1', bytes.fromhex(v['token'])))
        self.assertEqual(stored[33:].hex(), v['reply_key'])

    def test_fake_box_derives_the_same(self):
        v = vectors()
        key = bytes.fromhex(v['key'])
        self.assertEqual(
            fake_box.tagged_hash('OpenBMT/till-bearer/v1', key).hex(), v['token'])
        self.assertEqual(
            fake_box.tagged_hash('OpenBMT/till-reply/v1', key).hex(), v['reply_key'])

    def test_macs_of_the_cases(self):
        v = vectors()
        reply = bytes.fromhex(v['reply_key'])
        self.assertTrue(v['cases'])
        # The first case is a create, which has a body.
        self.assertTrue(v['cases'][0]['request_body'])
        for c in v['cases']:
            body = c['body'].encode('utf-8')
            args = (c['method'], c['path'], c['nonce'],
                    c['request_body'].encode('utf-8'), c['status'], body)
            self.assertEqual(mod.reply_mac(reply, *args), c['mac'])
            self.assertEqual(fake_box.mac_of(reply, *args), c['mac'])
            self.assertTrue(mod.reply_is_good(reply, *args, c['mac']))

    def test_a_case_changed_in_any_part_is_not_good(self):
        v = vectors()
        reply = bytes.fromhex(v['reply_key'])
        c = v['cases'][0]
        base = [c['method'], c['path'], c['nonce'],
                c['request_body'].encode('utf-8'), c['status'],
                c['body'].encode('utf-8')]
        # The request's body counts: one byte more is another mac.
        other = [('PUT', 0), ('/till/v1/sales/x', 1), ('f' * 32, 2),
                 (c['request_body'].encode('utf-8') + b' ', 3),
                 (c['status'] + 1, 4), (c['body'].encode('utf-8') + b' ', 5)]
        for value, at in other:
            args = list(base)
            args[at] = value
            self.assertFalse(mod.reply_is_good(reply, *args, c['mac']), at)
        self.assertFalse(mod.reply_is_good(bytes(32), *base, c['mac']))
        for header in (None, '', c['mac'].upper(), c['mac'][:-1], c['mac'] + '0',
                       c['mac'] + ' ', '\u00e9' * 64):
            self.assertFalse(mod.reply_is_good(reply, *base, header), header)


class Signed(Base):
    """An answer counts only with the mac of its own request (BX-80)."""

    def unsigned(self, r):
        self.assertEqual(r, {'error': mod.ERR_UNSIGNED,
                             'words': mod.LOCAL_WORDS[mod.ERR_UNSIGNED]})
        self.assert_clean(r)

    def test_a_good_answer_is_accepted(self):
        r = self.start()
        self.assertEqual(r['state'], 'waiting')
        self.assertEqual(self.status(r['sale'])['state'], 'waiting')

    def test_a_refusal_the_box_signed_is_accepted_with_its_words(self):
        r = self.status('0' * 32)
        self.assertEqual(r['error'], 'tillLost')

    def test_each_failure_of_the_mac_is_a_failed_call(self):
        for mode in ('missing', 'other_key', 'other_nonce', 'other_status',
                     'other_body', 'other_path', 'other_method',
                     'other_request'):
            self.box.signing = mode
            self.unsigned(self.start(request='r-' + mode))
            self.unsigned(self.status('0' * 32))

    def test_a_paid_answer_without_a_good_mac_is_never_paid(self):
        self.box.signing = 'good'
        sale = self.start()['sale']
        self.box.set_state(sale, 'paid')
        for mode in ('missing', 'other_key', 'other_nonce', 'other_status',
                     'other_body', 'other_request'):
            self.box.signing = mode
            r = self.status(sale)
            self.assertNotIn('state', r, mode)
            self.unsigned(r)
        self.box.signing = 'good'
        self.assertEqual(self.status(sale)['state'], 'paid')

    def test_an_answer_for_the_state_the_till_asked_about_elsewhere(self):
        # An answer made for one request is not good for another: the same
        # paid answer, signed for the sale's own path, does not pass as the
        # answer to a request for another path.
        reply = mod.till_reply_key(fake_box.KEY)
        sale = 'a' * 32
        raw = json.dumps({'sale': sale, 'state': 'paid',
                          'words': {'heading': '', 'what': 'Paid in bitcoin.',
                                    'next': ''}}).encode()
        mac = mod.reply_mac(reply, 'GET', '/till/v1/sales/' + sale, '1' * 32,
                            b'', 200, raw)
        self.assertTrue(mod.reply_is_good(
            reply, 'GET', '/till/v1/sales/' + sale, '1' * 32, b'', 200, raw, mac))
        self.assertFalse(mod.reply_is_good(
            reply, 'GET', '/till/v1/sales/' + 'b' * 32, '1' * 32, b'', 200, raw, mac))
        self.assertFalse(mod.reply_is_good(
            reply, 'GET', '/till/v1/sales/' + sale, '2' * 32, b'', 200, raw, mac))

    def test_an_answer_sent_again_for_a_later_request_is_refused(self):
        # What a path party holds after seeing one answer: its body, status
        # and mac. The next request has its own nonce.
        sale = self.start()['sale']
        self.box.signing = 'other_nonce'
        self.unsigned(self.status(sale))
        self.assertEqual(len(set(self.box.nonces)), len(self.box.nonces))

    def test_every_request_has_a_fresh_nonce_and_the_token_not_the_key(self):
        for i in range(5):
            self.start(request='n%d' % i)
        self.assertEqual(len(set(self.box.nonces)), 5)
        for n in self.box.nonces:
            self.assertRegex(n, r'^[0-9a-f]{32}$')
        for _, _, auth in self.box.seen:
            self.assertEqual(auth, 'Bearer ' + mod.till_token(fake_box.KEY))
            self.assertNotIn(fake_box.KEY, auth)

    def test_an_unsigned_refusal_is_not_taken_either(self):
        # The Box's 401 for a key file of the earlier form, and its 404, 400
        # and 413 answers, carry no mac.
        w = {'heading': 'h', 'what': 'w', 'next': 'n'}
        self.box.mode = 'shape'
        self.box.signing = 'missing'
        for status, body in ((401, {'error': 'tillKeyOld', 'words': w}),
                             (404, {}), (400, {'error': 'tillBadRequest'}),
                             (413, {'error': 'tillTooLarge'})):
            self.box.shape = body
            self.box.status_code = status
            self.unsigned(self.status('a' * 32))

    def test_the_key_is_checked_before_a_call_leaves(self):
        before = len(self.box.seen)
        for key in ('', 'ab' * 31, 'AB' * 32, 'zz' * 32, 'ab' * 33):
            r = self.start(method=self.make(openbmt_till_key=key))
            self.assertEqual(r['error'], mod.ERR_SETUP, key)
        self.assertEqual(len(self.box.seen), before)

    def test_a_key_with_spaces_around_it_works(self):
        m = self.make(openbmt_till_key=' ' + fake_box.KEY + '\n')
        self.assertEqual(self.start(method=m)['state'], 'waiting')


class Bound(Base):
    """The request is bound into the answer, and the answer names what the
    Box took (BX-72, BX-73, BX-80)."""

    def test_the_mac_covers_the_bytes_that_were_sent(self):
        self.start()
        sale = self.start()['sale']
        self.status(sale)
        # The create and the cancel carry bodies, the status poll none.
        self.start(request='r-2')
        self.method.openbmt_cancel(sale)
        self.assertEqual([b[:1] for b in self.box.sent],
                         [b'{', b'{', b'', b'{', b'{'])
        self.assertEqual(self.box.sent[4], b'{}')
        body = json.loads(self.box.sent[0])
        self.assertEqual(body['amount'], '12.50')
        # Each answer passed its mac check, which the fake Box makes over
        # the bytes it received.
        for call in (lambda: self.start(request='r-3'),
                     lambda: self.status(sale),
                     lambda: self.method.openbmt_cancel(sale)):
            self.assertNotIn('error', call())

    def test_a_request_changed_on_its_way_is_refused(self):
        self.box.signing = 'other_request'
        r = self.start()
        self.assertEqual(r, {'error': mod.ERR_UNSIGNED,
                             'words': mod.LOCAL_WORDS[mod.ERR_UNSIGNED]})

    def test_a_mac_of_another_amount_does_not_verify(self):
        reply = mod.till_reply_key(fake_box.KEY)
        real = b'{"amount":"100.00"}'
        changed = b'{"amount":"10.00"}'
        answer = b'{"sale":"' + b'a' * 32 + b'","state":"waiting"}'
        mac = mod.reply_mac(reply, 'POST', '/till/v1/sales', '1' * 32, changed,
                            200, answer)
        self.assertTrue(mod.reply_is_good(
            reply, 'POST', '/till/v1/sales', '1' * 32, changed, 200, answer, mac))
        self.assertFalse(mod.reply_is_good(
            reply, 'POST', '/till/v1/sales', '1' * 32, real, 200, answer, mac))

    def test_the_words_of_the_mismatch_are_placeholders(self):
        words = mod.LOCAL_WORDS[mod.ERR_MISMATCH]
        self.assertEqual(
            words, {'heading': 'Box answer does not match',
                    'what': 'The Box took a sale that is not the one this till asked for, so the sale was cancelled and nothing was charged.',
                    'next': "Take the payment again. If it keeps happening, check the connection between Odoo's server and the Box."})

    def mismatch(self, r):
        self.assertEqual(r, {'error': mod.ERR_MISMATCH,
                             'words': mod.LOCAL_WORDS[mod.ERR_MISMATCH]})
        self.assert_clean(r)

    def test_a_create_that_echoes_another_amount_is_cancelled_and_failed(self):
        for i, change in enumerate((
                {'amount': '1.00'}, {'currency': 'USD'},
                {'request': 'other'}, {'reference': 'Order 2'})):
            self.box.echo = change
            request = 'echo-%d' % i
            self.mismatch(self.start(request=request))
            self.box.echo = {}
            sale = self.box.requests[request]
            self.assertEqual(self.box.sales[sale], 'cancelled', change)

    def test_a_create_that_leaves_a_field_out_is_a_mismatch(self):
        for name in ('amount', 'currency', 'request', 'reference'):
            self.box.mode = 'shape'
            self.box.status_code = 200
            sale = 'a' * 32
            shape = {'sale': sale, 'state': 'waiting', 'amount': '12.50',
                     'currency': 'EUR', 'request': 'req-1',
                     'reference': 'Order 00012-003-0004'}
            del shape[name]
            self.box.shape = shape
            self.mismatch(self.start())

    def test_every_status_poll_is_compared(self):
        sale = self.start()['sale']
        self.assertEqual(self.status(sale)['state'], 'waiting')
        for change in ({'amount': '1.00'}, {'amount': 'x'}, {'currency': 'USD'},
                       {'request': 'other'}, {'reference': 'Order 2'},
                       {'amount': 12.5}):
            self.box.echo = change
            self.mismatch(self.status(sale))
            # The sale was cancelled at the Box.
            self.assertEqual(self.box.sales[sale], 'cancelled', change)
            self.box.echo = {}
            self.box.set_state(sale, 'waiting')

    def test_a_paid_answer_with_another_amount_is_not_paid(self):
        sale = self.start()['sale']
        self.box.set_state(sale, 'paid')
        self.box.echo = {'amount': '10.00'}
        r = self.status(sale)
        self.mismatch(r)
        self.assertNotIn('state', r)
        # The Box keeps a paid sale paid when asked to cancel it.
        self.assertEqual(self.box.sales[sale], 'paid')

    def test_the_till_asks_with_what_it_sends_on_a_poll(self):
        sale = self.start()['sale']
        self.assertEqual(
            self.status(sale, amount='13.50')['error'], mod.ERR_MISMATCH)
        self.assertEqual(
            self.status(sale, request='other')['error'], mod.ERR_MISMATCH)

    def test_the_amount_is_compared_as_a_number_and_the_reference_as_cut(self):
        r = self.start(amount='12.5')
        self.assertEqual(r['state'], 'waiting')
        long = 'R' * 60 + '\u00e9'
        m = self.method
        r = m.openbmt_start('3.00', 'EUR', long, 'long-1')
        self.assertEqual(r['state'], 'waiting')
        self.assertEqual(
            m.openbmt_status(r['sale'], '3.00', 'EUR', long, 'long-1')['state'],
            'waiting')

    def test_a_cancel_that_fails_still_fails_the_payment(self):
        # The answer to the cancel carries no good mac; the payment fails
        # all the same.
        orig = self.box.mac_for

        def mac_for(h, status, raw):
            if h.path.endswith('/cancel'):
                return None
            return orig(h, status, raw)

        self.box.mac_for = mac_for
        self.box.echo = {'amount': '1.00'}
        self.mismatch(self.start(request='nocancel'))
        self.assertTrue(self.box.seen[-1][1].endswith('/cancel'))


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
