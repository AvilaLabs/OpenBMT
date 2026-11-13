# Checked against: odoo/odoo 18.0 (run in a real server): the
# _get_payment_terminal_selection hook is a plain method returning a list, at
# addons/point_of_sale/models/pos_payment_method.py line 11 (used by the
# use_payment_terminal field, line 52); addons/pos_paytm/models/
# pos_payment_method.py line 41 extends it the same way. The fields the POS
# frontend loads are the fixed list in _load_pos_data_fields (same file, line
# 72), so none of the openbmt_* fields reach the browser.
import hashlib
import hmac
import json
import logging
import re
import secrets
from decimal import Decimal, InvalidOperation
from urllib.parse import urlsplit

import requests

from odoo import api, fields, models
from odoo.exceptions import ValidationError

_logger = logging.getLogger(__name__)

TIMEOUT = 10
MAX_ANSWER = 16 * 1024
MAX_REFERENCE = 40
SALE_RE = re.compile(r'^[0-9a-f]{32}$')
COUNTER_RE = re.compile(r'^[0-9a-f]{8}$')
REQUEST_RE = re.compile(r'^[A-Za-z0-9_-]{1,64}$')
AMOUNT_RE = re.compile(r'^[0-9]+(\.[0-9]+)?$')
CURRENCY_RE = re.compile(r'^[A-Z]{3}$')
STATES = ('waiting', 'opened', 'paid', 'unpaid', 'expired', 'cancelled')
ENDED = ('paid', 'unpaid', 'expired', 'cancelled')
KEY_RE = re.compile(r'^[0-9a-f]{64}$')

# The till key is never sent (docs/BOX-RULES.md BX-80): the module sends a
# token made from it and checks the mac of each answer under a reply key made
# from it. The mac covers the request's body too, and the answers that name a
# sale carry the amount, currency, request and reference the Box took, which
# the module checks against what it sent (BX-72, BX-73, BX-80). Tagged hashes
# are made as BIP340 makes them.
TAG_BEARER = 'OpenBMT/till-bearer/v1'
TAG_REPLY = 'OpenBMT/till-reply/v1'
REPLY_LABEL = 'OBM-TILL-REPLY-2'
REPLY_HEADER = 'X-OBM-Reply'
NONCE_HEADER = 'X-OBM-Nonce'


def tagged_hash(tag, message):
    prefix = hashlib.sha256(tag.encode('ascii')).digest()
    return hashlib.sha256(prefix + prefix + message).digest()


def till_token(key_hex):
    """The bearer token for a till key: 64 lowercase hex."""
    return tagged_hash(TAG_BEARER, bytes.fromhex(key_hex)).hex()


def till_reply_key(key_hex):
    """The key the Box signs its answers with, as 32 bytes."""
    return tagged_hash(TAG_REPLY, bytes.fromhex(key_hex))


def reply_mac(reply_key, method, path, nonce, sent, status, body):
    """The mac of an answer: 64 lowercase hex. `sent` is the bytes of the
    request's body (none for a request without one) and `body` the bytes of
    the answer's."""
    head = '%s\n%s\n%s\n%s\n%s\n%03d\n' % (
        REPLY_LABEL, method, path, nonce, hashlib.sha256(sent).hexdigest(),
        status)
    return hmac.new(reply_key, head.encode('utf-8') + body,
                    hashlib.sha256).hexdigest()


def reply_is_good(reply_key, method, path, nonce, sent, status, body, header):
    """Whether `header` is the mac of this answer to this request."""
    if not isinstance(header, str):
        return False
    want = reply_mac(reply_key, method, path, nonce, sent, status, body)
    return hmac.compare_digest(header.encode('utf-8'), want.encode('ascii'))


# Errors this module finds itself, before or instead of an answer from the
# Box, with the words the cashier sees.
ERR_TRANSPORT = 'tillTransport'
ERR_ANSWER = 'tillBadAnswer'
ERR_SETUP = 'tillNotSet'
ERR_UNSIGNED = 'tillNotSigned'
ERR_MISMATCH = 'tillMismatch'
LOCAL_WORDS = {
    ERR_TRANSPORT: {
        'heading': 'The till cannot reach the Box',
        'what': "Odoo's server got no answer from the Box address in this "
                'payment method.',
        'next': 'Check that the Box is on, that its till listener is started '
                'and that the address is right. Until then, take the payment '
                'on the counter and pick Bitcoin by hand.',
    },
    ERR_ANSWER: {
        'heading': 'The Box gave an answer the till cannot read',
        'what': 'The answer from the Box address is not one the till '
                "expects. The address may point at something other than the "
                "Box's till listener.",
        'next': 'Check the Box address in this payment method. Until then, '
                'take the payment on the counter and pick Bitcoin by hand.',
    },
    ERR_SETUP: {
        'heading': 'This payment method is not set up',
        'what': 'The Box address, till key or counter code is missing or '
                "wrong, or the sale's amount cannot be sent.",
        'next': 'Check the three OpenBMT fields of this payment method.',
    },
    ERR_UNSIGNED: {
        'heading': "The till cannot trust the Box's answer",
        'what': "The answer did not carry the Box's signature for the till "
                'key in this payment method. The till key may be wrong or '
                'made before a Box update, or something between Odoo\'s '
                'server and the Box changed the answer.',
        'next': 'Copy the till key from the Box again into this payment '
                'method. If the Box was updated, make a new key on it first. '
                'Until then, take the payment on the counter and pick '
                'Bitcoin by hand.',
    },
    ERR_MISMATCH: {
        'heading': 'Box answer does not match',
        'what': 'The Box took a sale that is not the one this till asked for, so the sale was cancelled and nothing was charged.',
        'next': "Take the payment again. If it keeps happening, check the connection between Odoo's server and the Box.",
    },
}


class PosPaymentMethod(models.Model):
    _inherit = 'pos.payment.method'

    def _get_payment_terminal_selection(self):
        return super()._get_payment_terminal_selection() + [('openbmt', 'OpenBMT')]

    openbmt_box_address = fields.Char(
        string='Box address',
        help="The address the Box's till listener shows, such as "
             "http://192.168.1.20:8091. Odoo's server must be able to reach it.")
    openbmt_till_key = fields.Char(
        string='Till key',
        help='Made on the Box with: openbmt-box till-key',
        groups='point_of_sale.group_pos_manager')
    openbmt_counter = fields.Char(
        string='Counter code',
        help='The 8 letters and digits after "Counter" in the owner app, '
             'such as 1a2b3c4d.')

    @api.constrains('openbmt_counter')
    def _check_openbmt_counter(self):
        for method in self:
            code = method.openbmt_counter
            if code and not COUNTER_RE.match(code):
                raise ValidationError(
                    'The counter code is 8 lowercase letters and digits, '
                    'such as 1a2b3c4d.')

    # Calls made by static/src/payment_openbmt.js.

    def openbmt_start(self, amount, currency, reference, request):
        self.ensure_one()
        if not (isinstance(amount, str) and AMOUNT_RE.match(amount)
                and isinstance(currency, str) and CURRENCY_RE.match(currency)
                and isinstance(reference, str)
                and isinstance(request, str) and REQUEST_RE.match(request)):
            return {'error': ERR_SETUP, 'words': LOCAL_WORDS[ERR_SETUP]}
        counter = self.openbmt_counter or ''
        if not COUNTER_RE.match(counter):
            return {'error': ERR_SETUP, 'words': LOCAL_WORDS[ERR_SETUP]}
        body = {
            'counter': counter,
            'amount': amount,
            'currency': currency,
            'reference': reference,
            'request': request,
        }
        return self._openbmt_call(
            'POST', '/till/v1/sales', body, expect=body)

    def openbmt_status(self, sale, amount, currency, reference, request):
        self.ensure_one()
        if not (isinstance(sale, str) and SALE_RE.match(sale)
                and isinstance(amount, str) and AMOUNT_RE.match(amount)
                and isinstance(currency, str) and CURRENCY_RE.match(currency)
                and isinstance(reference, str)
                and isinstance(request, str) and REQUEST_RE.match(request)):
            return {'error': ERR_SETUP, 'words': LOCAL_WORDS[ERR_SETUP]}
        expect = {
            'amount': amount,
            'currency': currency,
            'reference': reference,
            'request': request,
        }
        return self._openbmt_call(
            'GET', '/till/v1/sales/' + sale, None, sale, expect)

    def openbmt_cancel(self, sale):
        self.ensure_one()
        if not (isinstance(sale, str) and SALE_RE.match(sale)):
            return {'error': ERR_SETUP, 'words': LOCAL_WORDS[ERR_SETUP]}
        return self._openbmt_call(
            'POST', '/till/v1/sales/' + sale + '/cancel', {}, sale)

    # One HTTP call from the server to the Box.

    def _openbmt_call(self, verb, path, body, sale=None, expect=None):
        base = self._openbmt_base()
        key = (self.sudo().openbmt_till_key or '').strip()
        if base is None or not KEY_RE.match(key):
            return {'error': ERR_SETUP, 'words': LOCAL_WORDS[ERR_SETUP]}
        nonce = secrets.token_hex(16)
        # The bytes sent are made here, so the mac is checked against the
        # hash of exactly them.
        sent = b'' if body is None else json.dumps(
            body, separators=(',', ':')).encode('utf-8')
        headers = {'Authorization': 'Bearer ' + till_token(key),
                   NONCE_HEADER: nonce,
                   'Accept': 'application/json'}
        if body is not None:
            headers['Content-Type'] = 'application/json'
        try:
            response = requests.request(
                verb, base + path, data=sent if body is not None else None,
                timeout=TIMEOUT, allow_redirects=False, stream=True,
                headers=headers)
            raw = self._openbmt_read(response)
        except requests.RequestException as exc:
            # The class name only: the text of an exception may carry
            # request details.
            _logger.warning('OpenBMT box call failed: %s', type(exc).__name__)
            return {'error': ERR_TRANSPORT, 'words': LOCAL_WORDS[ERR_TRANSPORT]}
        if raw is None:
            return {'error': ERR_ANSWER, 'words': LOCAL_WORDS[ERR_ANSWER]}
        # An answer counts only when it carries the mac of this request, so an
        # answer that is missing it, changed, made up or sent again is a call
        # that failed.
        if not reply_is_good(till_reply_key(key), verb, path, nonce, sent,
                             response.status_code, raw,
                             response.headers.get(REPLY_HEADER)):
            return {'error': ERR_UNSIGNED, 'words': LOCAL_WORDS[ERR_UNSIGNED]}
        result = self._openbmt_parse(
            response.status_code, raw, sale, expect)
        if result.get('error') == ERR_MISMATCH and result.get('sale'):
            # The Box made a sale that is not the one asked for: stop it at
            # the Box, and fail the payment whatever comes of the cancel.
            self._openbmt_call(
                'POST', '/till/v1/sales/' + result['sale'] + '/cancel',
                {}, result['sale'])
            result = {'error': ERR_MISMATCH,
                      'words': LOCAL_WORDS[ERR_MISMATCH]}
        return result

    def _openbmt_base(self):
        address = (self.openbmt_box_address or '').strip()
        try:
            parts = urlsplit(address)
            host = parts.hostname
            _ = parts.port
        except ValueError:
            return None
        if parts.scheme not in ('http', 'https') or not host:
            return None
        if parts.username or parts.password or parts.query or parts.fragment:
            return None
        if parts.path not in ('', '/'):
            return None
        return address.rstrip('/')

    @staticmethod
    def _openbmt_read(response):
        chunks = []
        size = 0
        for chunk in response.iter_content(4096):
            size += len(chunk)
            if size > MAX_ANSWER:
                response.close()
                return None
            chunks.append(chunk)
        return b''.join(chunks)

    def _openbmt_parse(self, status, raw, sale, expect=None):
        try:
            data = json.loads(raw.decode('utf-8'))
        except (UnicodeDecodeError, ValueError):
            return {'error': ERR_ANSWER, 'words': LOCAL_WORDS[ERR_ANSWER]}
        if not isinstance(data, dict):
            return {'error': ERR_ANSWER, 'words': LOCAL_WORDS[ERR_ANSWER]}
        if status == 200:
            return self._openbmt_sale(data, sale, expect)
        error = data.get('error')
        words = self._openbmt_words(data.get('words'))
        if (status in (400, 401, 404, 409, 429, 503)
                and isinstance(error, str) and error.startswith('till')
                and len(error) <= 40 and error.isalnum() and words):
            return {'error': error, 'words': words}
        return {'error': ERR_ANSWER, 'words': LOCAL_WORDS[ERR_ANSWER]}

    def _openbmt_sale(self, data, expected, expect=None):
        got = data.get('sale')
        state = data.get('state')
        if not isinstance(got, str) or not SALE_RE.match(got):
            return {'error': ERR_ANSWER, 'words': LOCAL_WORDS[ERR_ANSWER]}
        if expected is not None and got != expected:
            return {'error': ERR_ANSWER, 'words': LOCAL_WORDS[ERR_ANSWER]}
        if expect is not None and not self._openbmt_same(data, expect):
            return {'error': ERR_MISMATCH, 'sale': got,
                    'words': LOCAL_WORDS[ERR_MISMATCH]}
        if state not in STATES:
            return {'error': ERR_ANSWER, 'words': LOCAL_WORDS[ERR_ANSWER]}
        words = self._openbmt_words(data.get('words'))
        if state in ENDED:
            # A paid sale may carry its text as one plain line.
            if words is None:
                return {'error': ERR_ANSWER, 'words': LOCAL_WORDS[ERR_ANSWER]}
        elif 'words' in data and data['words'] is not None:
            return {'error': ERR_ANSWER, 'words': LOCAL_WORDS[ERR_ANSWER]}
        return {'state': state, 'sale': got, 'words': words}

    @staticmethod
    def _openbmt_same(data, expect):
        """Whether the answer names the amount, currency, request and
        reference that were sent (BX-72, BX-73). The amount is compared as a
        number, the reference as the Box cuts it."""
        for name in ('amount', 'currency', 'request', 'reference'):
            if not isinstance(data.get(name), str):
                return False
        try:
            if Decimal(data['amount']) != Decimal(expect['amount']):
                return False
        except InvalidOperation:
            return False
        reference = ''.join(
            c for c in expect['reference'] if ' ' <= c <= '~')[:MAX_REFERENCE]
        return (AMOUNT_RE.match(data['amount']) is not None
                and data['currency'] == expect['currency']
                and data['request'] == expect['request']
                and data['reference'] == reference)

    @staticmethod
    def _openbmt_words(words):
        if not isinstance(words, dict):
            return None
        out = {}
        for name in ('heading', 'what', 'next'):
            value = words.get(name)
            if value is None and name != 'what':
                value = ''
            if not isinstance(value, str) or len(value) > 500:
                return None
            out[name] = value
        if not out['what']:
            return None
        return out
