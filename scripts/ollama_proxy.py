#!/usr/bin/env python3
"""Small authenticated bridge from Tailscale Funnel to a local Ollama instance.

Binds only to loopback. Never expose Ollama's port 11434 directly.
"""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen
import hmac
import json
import os

HOST = "127.0.0.1"
PORT = int(os.environ.get("OLLAMA_PROXY_PORT", "11435"))
TOKEN = os.environ.get("OLLAMA_PROXY_TOKEN", "")
OLLAMA = os.environ.get("OLLAMA_URL", "http://127.0.0.1:11434").rstrip("/")
MAX_BODY = 1_000_000

class Handler(BaseHTTPRequestHandler):
    server_version = "SmartCampusOllamaProxy/1.0"

    def send_json(self, status, value):
        raw = json.dumps(value, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        if self.path == "/health":
            self.send_json(200, {"ok": True, "service": "ollama-proxy"})
        else:
            self.send_json(404, {"error": "not_found"})

    def do_POST(self):
        if self.path != "/api/chat":
            self.send_json(404, {"error": "not_found"})
            return
        supplied = self.headers.get("Authorization", "")
        expected = "Bearer " + TOKEN
        if not TOKEN or not hmac.compare_digest(supplied, expected):
            self.send_json(401, {"error": "unauthorized"})
            return
        try:
            size = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            size = MAX_BODY + 1
        if size < 1 or size > MAX_BODY:
            self.send_json(413, {"error": "invalid_body_size"})
            return
        body = self.rfile.read(size)
        try:
            payload = json.loads(body)
            if not isinstance(payload, dict) or payload.get("stream") is not False:
                self.send_json(400, {"error": "only_non_streaming_chat_is_allowed"})
                return
            req = Request(
                OLLAMA + "/api/chat",
                data=json.dumps(payload).encode("utf-8"),
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urlopen(req, timeout=120) as response:
                result = response.read(MAX_BODY + 1)
            if len(result) > MAX_BODY:
                self.send_json(502, {"error": "ollama_response_too_large"})
                return
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(result)))
            self.end_headers()
            self.wfile.write(result)
        except HTTPError as error:
            self.send_json(502, {"error": "ollama_http_error", "status": error.code})
        except (URLError, TimeoutError, ValueError, json.JSONDecodeError) as error:
            self.send_json(502, {"error": "ollama_unavailable", "detail": str(error)[:200]})

    def log_message(self, fmt, *args):
        print("[ollama-proxy] " + (fmt % args), flush=True)

if __name__ == "__main__":
    if not TOKEN:
        raise SystemExit("Set OLLAMA_PROXY_TOKEN to a long random secret before starting.")
    print(f"Authenticated Ollama proxy listening on http://{HOST}:{PORT}", flush=True)
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()
