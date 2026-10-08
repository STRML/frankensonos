"""A stand-in daemon for the E2E harness (rows 20 to 22).

    stub-daemon.py PORT

* GET /events is refused with the POLICY_DENIED body the real daemon sent a LAN caller before `events` was added to
  its allow list (row 20: the app must stay connected and say why).
* Two rooms behave like speakers, so the app can be tested against the timing of real hardware (row 22):
  - "Lag Room" answers a resume the way a Sonos does, measured on a real one: the first read after the command still
    says `paused`, the next says `transitioning`, then `playing`. A pause takes effect at once.
  - "Stuck Room" accepts a resume and never starts playing, so the app must not claim it plays forever.
"""
import json
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import unquote

DENIED = {
    "detail": "unknown may not use events: unknown may only use: list_zones, play, pause",
    "code": "POLICY_DENIED",
    "hint": "The house policy forbids this; ask the owner to change it.",
    "suggestions": [],
    "retryable": False,
}
LOCK = threading.Lock()
# reads since the last resume; None when no resume is pending
PENDING = {"Lag Room": None, "Stuck Room": None}
TRANSPORT = {"Lag Room": "paused", "Stuck Room": "paused"}


def transport(room, count_read):
    with LOCK:
        pending = PENDING[room]
        if room == "Lag Room" and pending is not None:
            if count_read:
                PENDING[room] = pending + 1
            sequence = ["paused", "transitioning", "playing"]
            state = sequence[min(pending, 2)]
            if state == "playing":
                TRANSPORT[room] = "playing"
            return state
        return TRANSPORT[room]


def zone(room):
    return {"coordinator_room": room, "members": [room], "transport_state": transport(room, False), "household": "S2"}


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
        if path == "/zones":
            self.reply(200, [zone(room) for room in PENDING])
        elif path == "/rooms":
            self.reply(200, [{"name": room, "household": "S2", "zone": room} for room in PENDING])
        elif path.startswith("/zones/") and path.endswith("/state"):
            room = unquote(path[len("/zones/"):-len("/state")])
            if room.split("@")[0] not in PENDING:
                self.reply(404, {"detail": "unknown room", "code": "UNKNOWN_ROOM"})
                return
            room = room.split("@")[0]
            self.reply(200, {"zone": zone(room), "transport_state": transport(room, True), "volume": 20})
        elif path == "/health":
            self.reply(200, {"status": "ok", "version": "stub"})
        elif path == "/events":
            self.reply(403, DENIED)
        else:
            self.reply(404, {"detail": "not found", "code": "NOT_FOUND"})

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        body = json.loads(self.rfile.read(length) or b"{}")
        room = str(body.get("zone", "")).split("@")[0]
        path = self.path.split("?")[0]
        if room not in PENDING or path not in ("/resume", "/pause"):
            self.reply(404, {"detail": "not found", "code": "NOT_FOUND"})
            return
        with LOCK:
            if path == "/resume":
                PENDING[room] = 0 if room == "Lag Room" else None
            else:
                PENDING[room] = None
                TRANSPORT[room] = "paused"
        self.reply(200, {"done": path[1:] + "d " + room, "changed": True})

    def log_message(self, *args):
        pass


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
