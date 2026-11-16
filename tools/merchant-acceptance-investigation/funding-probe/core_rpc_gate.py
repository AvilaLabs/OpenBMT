#!/usr/bin/env python3
"""Loopback Core RPC proxy; suppress a selected provider tx and its retries."""
import base64
import json
import os
import threading
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

UPSTREAM = int(os.environ.get("CORE_RPC_UPSTREAM", "19700"))
LISTEN = int(os.environ.get("CORE_RPC_GATE_PORT", "19714"))
RPC_USER = os.environ.get("CORE_RPC_USER", "shopln")
RPC_PASSWORD = os.environ.get("CORE_RPC_PASS", "shopln_pw")
GATE = Path(os.environ["CORE_RPC_GATE_FILE"])
MARKER = Path(os.environ["CORE_RPC_MARKER_FILE"])
LOCK = threading.Lock()


def upstream_call(method, params):
    body = json.dumps({"jsonrpc": "1.0", "id": "gate", "method": method, "params": params}).encode()
    auth = base64.b64encode(f"{RPC_USER}:{RPC_PASSWORD}".encode()).decode()
    req = urllib.request.Request(
        f"http://127.0.0.1:{UPSTREAM}/", body,
        {"Content-Type": "application/json", "Authorization": f"Basic {auth}"},
    )
    with urllib.request.urlopen(req, timeout=30) as response:
        payload = json.loads(response.read())
    if payload.get("error") is not None:
        raise RuntimeError(f"Core {method} error: {payload['error']}")
    return payload.get("result")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        print("HTTP " + (fmt % args), flush=True)

    def do_POST(self):
        try:
            request = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))))
            is_batch = isinstance(request, list)
            requests = request if is_batch else [request]
            replies = []
            for item in requests:
                method = item.get("method", "")
                params = item.get("params", [])
                ident = item.get("id")
                if method == "sendrawtransaction" and (GATE.exists() or MARKER.exists()):
                    with LOCK:
                        # Decode every gated candidate and then suppress this exact
                        # txid forever, including retries after an apparent RPC success.
                        raw_tx = params[0] if params else ""
                        decoded = upstream_call("decoderawtransaction", [raw_tx])
                        txid = decoded["txid"]
                        selected = MARKER.read_text().strip() if MARKER.exists() else ""
                        if not selected and GATE.exists():
                            MARKER.write_text(txid + "\n")
                            selected = txid
                        if txid == selected:
                            print(f"SUPPRESSED sendrawtransaction txid={txid}", flush=True)
                            replies.append({"result": txid, "error": None, "id": ident})
                            continue
                result = upstream_call(method, params)
                replies.append({"result": result, "error": None, "id": ident})
            output = replies if is_batch else replies[0]
            raw = json.dumps(output).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)
        except Exception as exc:
            raw = json.dumps({"result": None, "error": {"code": -32603, "message": str(exc)}, "id": None}).encode()
            self.send_response(500)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)


print(f"RPC gate listening 127.0.0.1:{LISTEN}, forwarding to Core 127.0.0.1:{UPSTREAM}", flush=True)
ThreadingHTTPServer(("127.0.0.1", LISTEN), Handler).serve_forever()
