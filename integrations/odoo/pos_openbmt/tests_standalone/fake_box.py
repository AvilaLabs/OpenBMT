"""A Box till listener built from the HTTP contract, on http.server."""
import hashlib
import hmac
import json
import re
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

KEY = 'ab' * 32
COUNTER = '1a2b3c4d'
LIMIT = 4096
MAX_WAITING = 32

WORDS = {
    'tillKeyWrong': ('Wrong till key',
                     "The key in this payment method is not the Box's till key.",
                     "Copy the till key from the Box again into this payment method."),
    'tillNotPaired': ('The Box is not set up yet',
                      'The Box has not joined the shop, so it cannot reach a counter.',
                      'Finish setting up the Box from the owner app.'),
    'tillUnknownCounter': ('Counter not found',
                           'The shop has no counter with the code 00000000.',
                           'Check the counter code in this payment method. '
                           'The owner app shows it after the word Counter.'),
    'tillWrongMoney': ('Wrong currency',
                       "This sale is in USD, and the shop's counters take EUR.",
                       'Take this payment on the counter by hand.'),
    'tillBadAmount': ('This amount cannot be taken',
                      'The amount is zero, below zero, or has more decimals than the money has.',
                      'Check the sale in the till.'),
    'tillBusy': ('Too many sales are waiting',
                 '32 sales from the till are waiting for a counter.',
                 'Finish or cancel some of them, then try again.'),
    'tillLost': ('The Box restarted',
                 'The Box no longer knows this sale.',
                 'Check the counter.'),
}
END_WORDS = {
    'paid': ('', 'Paid in bitcoin.', ''),
    'unpaid': ('The payment did not arrive',
               "The counter's sale ended without being paid.",
               'Ask the customer to try again, or pick another way to pay.'),
    'expired': ('Sale not started',
                'Nobody pressed Start on the counter within 5 minutes.',
                'Try again.'),
    'cancelled': ('Sale cancelled',
                  'The till cancelled this sale before it was paid.',
                  'Nothing to do.'),
}
NONCE_RE = re.compile(r'^[0-9a-f]{32}$')
ID_RE = re.compile(r'^[A-Za-z0-9_-]{1,64}$')
AMOUNT_RE = re.compile(r'^[0-9]+(\.[0-9]{1,2})?$')


def tagged_hash(tag, message):
    prefix = hashlib.sha256(tag.encode('ascii')).digest()
    return hashlib.sha256(prefix + prefix + message).digest()


def mac_of(reply_key, method, path, nonce, sent, status, body):
    message = ('OBM-TILL-REPLY-2\n%s\n%s\n%s\n%s\n%03d\n'
               % (method, path, nonce, hashlib.sha256(sent).hexdigest(),
                  status)).encode() + body
    return hmac.new(reply_key, message, hashlib.sha256).hexdigest()


def words(trio):
    return {'heading': trio[0], 'what': trio[1], 'next': trio[2]}


class FakeBox:
    def __init__(self):
        key = bytes.fromhex(KEY)
        # What the Box keeps of the till key: the hash of the bearer token,
        # and the reply key.
        self.key_hash = tagged_hash(
            'OpenBMT/till-key/v1', tagged_hash('OpenBMT/till-bearer/v1', key))
        self.reply_key = tagged_hash('OpenBMT/till-reply/v1', key)
        # How answers are signed: good, missing, other_key, other_nonce,
        # other_status, other_body, other_path, other_method or
        # other_request (the mac of a request with another body).
        self.signing = 'good'
        self.paired = True
        self.sales = {}
        # What the Box took for each sale: amount, currency, request and
        # reference.
        self.info = {}
        # Fields of the answers told wrong, as a Box that lies would (the
        # mac stays good), and the bodies the Box was sent.
        self.echo = {}
        self.sent = []
        self.requests = {}
        self.seen = []  # (method, path, authorization)
        self.nonces = []
        self.mode = 'normal'  # normal, redirect, garbage, slow, shape
        self.delay = 0.0
        # In drip mode: seconds between the bytes of the answer.
        self.gap = 1.0
        self.shape = None
        self.status_code = 200
        self.currency = 'EUR'
        # When set, each status read moves a sale one step: waiting, opened, paid.
        self.advance = False
        self.lock = threading.Lock()
        box = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = 'HTTP/1.1'

            def log_message(self, *args):
                pass

            def do_GET(self):
                box.handle(self, 'GET')

            def do_POST(self):
                box.handle(self, 'POST')

            def do_PUT(self):
                box.handle(self, 'PUT')

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.address = 'http://127.0.0.1:%d' % self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()

    def set_state(self, sale, state):
        with self.lock:
            self.sales[sale] = state

    def mac_for(self, h, status, raw):
        method, path, nonce, sent = h.command, h.path, h.nonce, h.sent
        key = self.reply_key
        mode = self.signing
        if mode == 'missing':
            return None
        if mode == 'other_key':
            key = bytes(32)
        elif mode == 'other_nonce':
            nonce = '0' * 32 if nonce != '0' * 32 else '1' * 32
        elif mode == 'other_status':
            status = 200 if status != 200 else 404
        elif mode == 'other_body':
            raw = raw + b' '
        elif mode == 'other_path':
            path = '/till/v1/other'
        elif mode == 'other_method':
            method = 'PUT' if method != 'PUT' else 'GET'
        elif mode == 'other_request':
            sent = sent + b' '
        return mac_of(key, method, path, nonce, sent, status, raw)

    def send(self, h, status, body, extra=None, signed=True):
        raw = body if isinstance(body, bytes) else json.dumps(body).encode()
        h.send_response(status)
        h.send_header('Content-Type', 'application/json')
        h.send_header('Content-Length', str(len(raw)))
        mac = self.mac_for(h, status, raw) if signed and h.nonce else None
        if mac:
            h.send_header('X-OBM-Reply', mac)
        for k, v in (extra or {}).items():
            h.send_header(k, v)
        h.end_headers()
        h.wfile.write(raw)

    def drip(self, h):
        """Headers at once, then the body a byte at a time."""
        h.send_response(200)
        h.send_header('Content-Type', 'application/json')
        h.send_header('Content-Length', '4000')
        h.end_headers()
        h.wfile.flush()
        try:
            for _ in range(4000):
                h.wfile.write(b' ')
                h.wfile.flush()
                time.sleep(self.gap)
        except OSError:
            pass
        h.close_connection = True

    def view(self, sale, state):
        """A sale as the Box tells it, with what `echo` changes."""
        body = {'sale': sale, 'state': state}
        body.update(self.info.get(sale, {}))
        body.update(self.echo)
        return body

    def err(self, h, status, key):
        self.send(h, status, {'error': key, 'words': words(WORDS[key])})

    def handle(self, h, verb):
        nonce = h.headers.get('X-OBM-Nonce')
        h.nonce = nonce if nonce and NONCE_RE.match(nonce) else None
        length = int(h.headers.get('Content-Length') or 0)
        h.sent = b''
        if length > LIMIT:
            self.send(h, 413, {}, signed=False)
            return
        raw = h.rfile.read(length) if length else b''
        h.sent = raw
        auth = h.headers.get('Authorization')
        with self.lock:
            self.seen.append((verb, h.path, auth))
            self.sent.append(raw)
            self.nonces.append(nonce)
        if self.mode == 'redirect':
            self.send(h, 302, {}, {'Location': self.address + '/elsewhere'})
            return
        if self.mode == 'slow':
            time.sleep(self.delay)
        if self.mode == 'drip':
            self.drip(h)
            return
        if self.mode == 'garbage':
            self.send(h, 200, b'<html>not json</html>')
            return
        if self.mode == 'shape':
            self.send(h, self.status_code, self.shape)
            return
        path = h.path
        m_sale = re.match(r'^/till/v1/sales/([0-9a-f]{32})$', path)
        m_cancel = re.match(r'^/till/v1/sales/([0-9a-f]{32})/cancel$', path)
        known = (path == '/till/v1/sales' and verb == 'POST') \
            or (m_sale and verb == 'GET') or (m_cancel and verb == 'POST')
        if not known:
            self.send(h, 404, {}, signed=False)
            return
        if h.nonce is None:
            self.send(h, 400, {'error': 'tillBadRequest'}, signed=False)
            return
        if auth is None or not auth.startswith('Bearer '):
            self.err(h, 401, 'tillKeyWrong')
            return
        digest = tagged_hash('OpenBMT/till-key/v1', bytes.fromhex(auth[7:])
                             if re.match(r'^[0-9a-f]{64}$', auth[7:]) else b'x')
        if digest != self.key_hash:
            self.err(h, 401, 'tillKeyWrong')
            return
        if not self.paired:
            self.err(h, 503, 'tillNotPaired')
            return
        if path == '/till/v1/sales':
            self.create(h, raw)
        elif m_sale:
            self.status(h, m_sale.group(1))
        else:
            self.cancel(h, m_cancel.group(1))

    def create(self, h, raw):
        try:
            b = json.loads(raw)
            counter, amount, currency = b['counter'], b['amount'], b['currency']
            reference, request = b['reference'], b['request']
        except (ValueError, KeyError, TypeError):
            self.err(h, 400, 'tillBadAmount')
            return
        if counter != COUNTER:
            self.err(h, 404, 'tillUnknownCounter')
            return
        if not (isinstance(amount, str) and AMOUNT_RE.match(amount)
                and float(amount) > 0):
            self.err(h, 400, 'tillBadAmount')
            return
        if currency != self.currency:
            self.err(h, 409, 'tillWrongMoney')
            return
        if not (isinstance(request, str) and ID_RE.match(request)):
            self.err(h, 400, 'tillBadAmount')
            return
        with self.lock:
            if request in self.requests:
                sale = self.requests[request]
            else:
                waiting = sum(1 for s in self.sales.values()
                              if s in ('waiting', 'opened'))
                if waiting >= MAX_WAITING:
                    sale = None
                else:
                    sale = hashlib.sha256(request.encode()).hexdigest()[:32]
                    self.requests[request] = sale
                    self.sales[sale] = 'waiting'
                    self.info[sale] = {
                        'amount': '%.2f' % float(amount),
                        'currency': currency,
                        'request': request,
                        # Cut to 40 printable ASCII characters, as the Box does.
                        'reference': ''.join(
                            c for c in str(reference) if ' ' <= c <= '~')[:40],
                    }
        if sale is None:
            self.err(h, 429, 'tillBusy')
            return
        self.send(h, 200, self.view(sale, 'waiting'))

    def status(self, h, sale):
        with self.lock:
            state = self.sales.get(sale)
            if self.advance and state in ('waiting', 'opened'):
                state = self.sales[sale] = (
                    'opened' if state == 'waiting' else 'paid')
        if state is None:
            self.err(h, 404, 'tillLost')
            return
        body = self.view(sale, state)
        if state in END_WORDS:
            body['words'] = words(END_WORDS[state])
        self.send(h, 200, body)

    def cancel(self, h, sale):
        with self.lock:
            state = self.sales.get(sale)
            if state is None:
                pass
            elif state != 'paid':
                self.sales[sale] = state = 'cancelled'
        if state is None:
            self.err(h, 404, 'tillLost')
            return
        body = self.view(sale, state)
        body['words'] = words(END_WORDS[state])
        self.send(h, 200, body)
