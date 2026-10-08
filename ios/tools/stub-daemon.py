"""A stand-in daemon that answers like one whose house policy refuses the live stream.

    stub-daemon.py PORT

GET /zones and /rooms answer an empty house, /health answers ok, and GET /events is refused with the same
POLICY_DENIED body the real daemon sent a LAN caller before `events` was added to its allow list. Used by the E2E
harness (row 20) to prove the app stays connected and says why instead of sitting on "reconnecting".
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DENIED = {
    "detail": "unknown may not use events: unknown may only use: list_zones, play, pause",
    "code": "POLICY_DENIED",
    "hint": "The house policy forbids this; ask the owner to change it.",
    "suggestions": [],
    "retryable": False,
}


class Handler(BaseHTTPRequestHandler):
    def reply(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = self.path.split("?")[0]
        if path in ("/zones", "/rooms"):
            self.reply(200, [])
        elif path == "/health":
            self.reply(200, {"status": "ok", "version": "stub"})
        elif path == "/events":
            self.reply(403, DENIED)
        else:
            self.reply(404, {"detail": "not found", "code": "NOT_FOUND"})

    def log_message(self, *args):
        pass


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
