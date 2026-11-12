# Checked against: odoo/odoo 18.0, addons/pos_paytm/models/pos_payment_method.py
# (as quoted in the research notes). UNCONFIRMED there: the exact decorator
# and return shape of _get_payment_terminal_selection (written as paytm shows
# it: parent list plus one tuple).
import json
import logging
import re
from urllib.parse import urlsplit

import requests

from odoo import api, fields, models
from odoo.exceptions import ValidationError

_logger = logging.getLogger(__name__)

TIMEOUT = 10
MAX_ANSWER = 16 * 1024
SALE_RE = re.compile(r'^[0-9a-f]{32}$')
COUNTER_RE = re.compile(r'^[0-9a-f]{8}$')
REQUEST_RE = re.compile(r'^[A-Za-z0-9_-]{1,64}$')
AMOUNT_RE = re.compile(r'^[0-9]+(\.[0-9]+)?$')
CURRENCY_RE = re.compile(r'^[A-Z]{3}$')
STATES = ('waiting', 'opened', 'paid', 'unpaid', 'expired', 'cancelled')
ENDED = ('paid', 'unpaid', 'expired', 'cancelled')

# Keys of errors this module raises itself; the words file has no text for
# them, so no words travel with them.
ERR_TRANSPORT = 'tillTransport'
ERR_ANSWER = 'tillBadAnswer'
ERR_SETUP = 'tillNotSet'


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
            return {'error': ERR_SETUP, 'words': None}
        counter = self.openbmt_counter or ''
        if not COUNTER_RE.match(counter):
            return {'error': ERR_SETUP, 'words': None}
        body = {
            'counter': counter,
            'amount': amount,
            'currency': currency,
            'reference': reference,
            'request': request,
        }
        return self._openbmt_call('POST', '/till/v1/sales', body)

    def openbmt_status(self, sale):
        self.ensure_one()
        if not (isinstance(sale, str) and SALE_RE.match(sale)):
            return {'error': ERR_SETUP, 'words': None}
        return self._openbmt_call('GET', '/till/v1/sales/' + sale, None, sale)

    def openbmt_cancel(self, sale):
        self.ensure_one()
        if not (isinstance(sale, str) and SALE_RE.match(sale)):
            return {'error': ERR_SETUP, 'words': None}
        return self._openbmt_call(
            'POST', '/till/v1/sales/' + sale + '/cancel', {}, sale)

    # One HTTP call from the server to the Box.

    def _openbmt_call(self, verb, path, body, sale=None):
        base = self._openbmt_base()
        key = self.sudo().openbmt_till_key
        if base is None or not key:
            return {'error': ERR_SETUP, 'words': None}
        try:
            response = requests.request(
                verb, base + path, json=body, timeout=TIMEOUT,
                allow_redirects=False, stream=True,
                headers={'Authorization': 'Bearer ' + key,
                         'Accept': 'application/json'})
            raw = self._openbmt_read(response)
        except requests.RequestException as exc:
            # The class name only: the text of an exception may carry
            # request details.
            _logger.warning('OpenBMT box call failed: %s', type(exc).__name__)
            return {'error': ERR_TRANSPORT, 'words': None}
        if raw is None:
            return {'error': ERR_ANSWER, 'words': None}
        return self._openbmt_parse(response.status_code, raw, sale)

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

    def _openbmt_parse(self, status, raw, sale):
        try:
            data = json.loads(raw.decode('utf-8'))
        except (UnicodeDecodeError, ValueError):
            return {'error': ERR_ANSWER, 'words': None}
        if not isinstance(data, dict):
            return {'error': ERR_ANSWER, 'words': None}
        if status == 200:
            return self._openbmt_sale(data, sale)
        error = data.get('error')
        words = self._openbmt_words(data.get('words'))
        if (status in (400, 401, 404, 409, 429, 503)
                and isinstance(error, str) and error.startswith('till')
                and len(error) <= 40 and error.isalnum() and words):
            return {'error': error, 'words': words}
        return {'error': ERR_ANSWER, 'words': None}

    def _openbmt_sale(self, data, expected):
        got = data.get('sale')
        state = data.get('state')
        if not isinstance(got, str) or not SALE_RE.match(got):
            return {'error': ERR_ANSWER, 'words': None}
        if expected is not None and got != expected:
            return {'error': ERR_ANSWER, 'words': None}
        if state not in STATES:
            return {'error': ERR_ANSWER, 'words': None}
        words = self._openbmt_words(data.get('words'))
        if state in ENDED:
            # A paid sale may carry its text as one plain line.
            if words is None:
                return {'error': ERR_ANSWER, 'words': None}
        elif 'words' in data and data['words'] is not None:
            return {'error': ERR_ANSWER, 'words': None}
        return {'state': state, 'sale': got, 'words': words}

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
