# Deploying FrankenSonos (launchd + Tailscale)

This guide runs the `fsonos` daemon on an always-on Mac on the speaker LAN
(for example a Mac mini). launchd keeps it running across reboots and crashes,
and Tailscale lets your agents reach it from anywhere on your tailnet. The
speakers stay on the LAN and are never fronted.

> **Status.** `fsonos serve` runs the HTTP API and the MCP server (streamable
> HTTP at `/mcp`) over the house policy, and keeps a live model of the
> speakers from their GENA events, verified end to end against the built-in
> simulator. Spotify sign-in and background sync populate the cache the DJ
> reads on each start.

## 1. Shape of the deployment

```text
 tailnet device (agent, phone, laptop)
        │  HTTPS over WireGuard, admitted by your tailnet policy
        ▼
 Tailscale Serve on the Mac ── https://<mac>.<tailnet>.ts.net      → 127.0.0.1:8099  HTTP API
                            └─ https://<mac>.<tailnet>.ts.net:8443 → 127.0.0.1:8098  MCP (/mcp)
        │
 fsonos serve (launchd)
        │  SSDP multicast, SOAP to :1400        ▲  GENA NOTIFY event callbacks
        ▼                                       │  (speakers → daemon, LAN only)
 Sonos players on the LAN ──────────────────────┘
```

Six rules hold the design together:

1. **Reachability is control.** The HTTP API and the MCP server have no
   authentication of their own: anyone who can reach them can play, pause and
   regroup your speakers. They bind **loopback**, and Tailscale Serve plus your
   tailnet policy decide who else gets in.
2. **Tailscale fronts the daemon, never the speakers.** Do not advertise the
   speaker LAN as a subnet route (`tailscale set --advertise-routes=…`). That
   would put every speaker's unauthenticated port 1400 on the tailnet.
3. **Never `tailscale funnel`** these ports. Funnel publishes to the public
   internet.
4. **The GENA callback listener is the only LAN-facing socket.** The speakers
   must be able to reach the daemon to deliver state-change events. It accepts
   event deliveries only, not control requests, on `FSONOS_EVENTS_PORT`
   (default 8097) at the Mac's address facing the speakers.
5. **Browsers can't drive it.** A web page open on the Mac or a tailnet
   device could otherwise reach the unauthenticated API. So each listener
   admits only its own Host names (loopback, its address, the tailnet's
   MagicDNS name and addresses), defeating DNS rebinding. A request with a
   foreign `Origin` gets 403, and every control request must be
   `Content-Type: application/json` (415 otherwise), which closes the
   no-preflight cross-origin POST. No CORS grant is ever sent. CLI tools and
   agents send no `Origin` and are unaffected.
6. **No site data in git.** Filled-in plists, tailnet policies, seed lists,
   logs and the data directory all live outside the repository.

## 2. Configuration

`fsonos serve` reads its configuration from flags or the matching environment
variables. The environment form is what launchd uses.

| Setting | Env var | Default | Notes |
|---|---|---|---|
| HTTP API address | `FSONOS_HTTP_ADDR` | unset: `127.0.0.1:8099` plus this host's tailnet addresses when Tailscale is up | Set it to bind exactly these addresses: one, e.g. loopback behind Tailscale Serve, or a comma-separated list such as `127.0.0.1:8099,192.168.1.20:8099` to serve the LAN and keep loopback for on-host tools and the Spotify sign-in tunnel (callers on a LAN address are `unknown`; the sign-in routes only answer loopback callers). |
| MCP (streamable HTTP) address | `FSONOS_MCP_HTTP_ADDR` | unset: `127.0.0.1:8098` | Endpoint path `/mcp`. Loopback unless set: reach it from the tailnet through Serve. |
| Tailscale detection | `FSONOS_TAILSCALE` | `auto` | `off` (or `--tailscale off`) stops `fsonos` looking for Tailscale: unconfigured listeners bind loopback only, and `fsonos doctor` skips its `tailscale.*` checks. |
| Tailscale Serve at startup | `FSONOS_TAILSCALE_SERVE` | off | `true` (or `--tailscale-serve`) runs `fsonos tailscale setup` when the daemon starts (§ "Recommended: Tailscale Serve"); a refusal or failure is logged and the daemon serves on. |
| Events port | `FSONOS_EVENTS_PORT` | `8097` | Where the speakers deliver state-change events (GENA), on the Mac's LAN address. The only LAN-facing socket: allow it inbound (§4). `0` picks any free port. |
| Data directory | `FSONOS_DATA_DIR` | `~/Library/Application Support/fsonos` | Store DB and Spotify token cache. |
| Direct-seed list | `FSONOS_SEEDS` | unset | Optional file of player addresses for flaky-SSDP networks; every IP address in it is tried (e.g. TOML `players = ["192.0.2.10"]`, or one per line). Every command also takes `--seed <ip>`. Keep the file under `local/` or outside the repo. |
| Routes file | `FSONOS_ROUTES` | unset | Only for `fsonos sim`: maps the virtual players' advertised addresses to the loopback sockets that serve them, plus the simulator's SSDP target. `fsonos sim` writes it; real players need none. While it is set, `fsonos` reaches nothing the file does not name (other addresses and multicast are refused). |
| Spotify client id | `FSONOS_SPOTIFY_CLIENT_ID` | unset | Needed for Spotify sign-in, library browsing and the DJ (see §5). |
| Spotify app redirect URI | `FSONOS_SPOTIFY_APP_REDIRECT_URI` | `frankensonos://spotify-callback` | `--spotify-app-redirect-uri`; register this exact URI for phone sign-in. |
| Spotify accounts base | `FSONOS_SPOTIFY_ACCOUNTS_URL` | unset | Tests and fakes only; overrides the accounts base, including authorization and token exchange. |
| Spotify API base | `FSONOS_SPOTIFY_API_URL` | unset | Tests and fakes only; overrides the Web API base, including `/v1`. |
| Spotify redirect URI | `FSONOS_SPOTIFY_REDIRECT_URI` | `http://127.0.0.1:8099/auth/spotify/callback` | Must match the URI registered for your Spotify app. |
| Log filter | `RUST_LOG` | `info` | `tracing` EnvFilter syntax. Logs go to stderr. |

**Bind guard**: `serve` refuses a wildcard
(`0.0.0.0`, `::`) or public bind address for the API or MCP server unless you
pass `--allow-unsafe-bind`. It logs a warning for a private-LAN address.
Loopback and tailnet (`100.64.0.0/10`, `fd7a:115c:a1e0::/48`) addresses are
accepted silently.

## 3. Build and install

Install to one stable path. The launchd job, the macOS firewall and the
privacy records all key on the executable's location.

```bash
cargo build --release -p fsonos-cli          # or offload via rch
install -d ~/.local/bin
install -m 0755 target/release/fsonos ~/.local/bin/fsonos
~/.local/bin/fsonos --version
mkdir -p ~/Library/Logs/fsonos "$HOME/Library/Application Support/fsonos"
```

To upgrade later, rebuild, re-run the `install` line, and restart the job with
`kickstart -k` (§4).

## 4. Run it under launchd

### Why a LaunchDaemon (recommended)

On macOS 15 and later, **Local Network privacy** gates the daemon's core work:
SSDP multicast and SOAP connections to the speakers. Per Apple's
[TN3179](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy),
macOS automatically allows local-network access for:

- launchd **daemons**;
- processes running as root;
- command-line tools run from Terminal or over SSH, including their children.

The exemption does **not** cover launchd **agents**. An agent gets the Local
Network alert, and its LAN traffic is blocked until someone approves it. A
headless Mac can't answer that prompt. Approval is also tracked by code
signature, which is unreliable for the ad-hoc-signed binaries `cargo`
produces, so a rebuild can trigger the prompt again.

A **LaunchDaemon** with a `UserName` key avoids all of that. It starts at boot
without anyone logging in and runs as your user, not root. Inbound connections
(the speakers' event callbacks) need no Local Network privilege in either mode.

### Install the LaunchDaemon

The template lives at
[`docs/launchd/io.github.dicklesworthstone.fsonos.plist`](launchd/io.github.dicklesworthstone.fsonos.plist).
launchd does not expand `~`, so fill in absolute paths:

```bash
LABEL=io.github.dicklesworthstone.fsonos
sed -e "s|__USER__|$(id -un)|g" -e "s|__HOME__|$HOME|g" \
    docs/launchd/$LABEL.plist > "${TMPDIR:-/tmp}/$LABEL.plist"
plutil -lint "${TMPDIR:-/tmp}/$LABEL.plist"
# LaunchDaemon plists must be root:wheel and not group/world-writable.
sudo install -m 0644 -o root -g wheel "${TMPDIR:-/tmp}/$LABEL.plist" /Library/LaunchDaemons/
sudo launchctl bootstrap system /Library/LaunchDaemons/$LABEL.plist
```

Day-to-day management:

```bash
sudo launchctl print system/$LABEL | grep -E 'state|pid|last exit'
sudo launchctl kickstart -k system/$LABEL     # restart (e.g. after an upgrade)
sudo launchctl bootout system/$LABEL          # stop and unload
tail -f ~/Library/Logs/fsonos/fsonos.log
```

The job sets `KeepAlive` to restart on any exit, at most once every 10 s
(`ThrottleInterval`). It also gets 20 s between SIGTERM and SIGKILL
(`ExitTimeOut`) so the daemon can cancel its event subscriptions cleanly.

### Alternative: a LaunchAgent

Use a LaunchAgent if you want the daemon to run only while you are logged in.
Delete the `UserName` key from the filled-in plist, put it in
`~/Library/LaunchAgents/`, and manage it in the `gui/$(id -u)` domain:

```bash
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/$LABEL.plist
launchctl kickstart -k gui/$(id -u)/$LABEL
```

Approve the Local Network prompt on first run (System Settings → Privacy &
Security → Local Network). `serve` is designed to keep running when discovery
fails, and that matters: TN3179 notes that macOS skips the alert for a process
that exits right after its first failed local-network operation. Expect to
re-approve after rebuilds. The Mac also needs a logged-in GUI session (auto-login) after
a power cut.

### Foreground (development)

`fsonos serve` in Terminal or over SSH needs no launchd and is automatically
allowed local-network access.

### macOS Application Firewall

If the firewall is on, it can block the speakers' event callbacks, and live
state then goes stale. This is a separate gate from Local Network privacy.
Allow the binary:

```bash
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add ~/.local/bin/fsonos
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --unblockapp ~/.local/bin/fsonos
```

A port-based firewall (pf, or a third-party one) must admit inbound TCP on
the events port (`FSONOS_EVENTS_PORT`, default 8097) from the speakers'
subnet. Nothing else needs to be reachable from the LAN.

## 5. Tailscale

### Recommended: Tailscale Serve in front of loopback

Enable **MagicDNS** and **HTTPS certificates** in the Tailscale admin console
(DNS page). Then, on the Mac:

```bash
fsonos tailscale setup      # both mappings below; running it again changes nothing
fsonos tailscale status     # what Serve does on 443 / 8443, and the URLs
fsonos tailscale teardown   # remove the daemon's mappings (only those)
```

`setup` refuses while **Funnel** is on for either port (Funnel would publish
the unauthenticated API to the public internet; FrankenSonos never uses it),
and never replaces Serve config it did not make. `--dry-run` shows the
commands. By hand, the same thing is:

```bash
tailscale serve --bg --https=443  http://127.0.0.1:8099   # HTTP API
tailscale serve --bg --https=8443 http://127.0.0.1:8098   # MCP
tailscale serve status
```

- The API is at `https://<mac>.<tailnet>.ts.net/` and the MCP endpoint at
  `https://<mac>.<tailnet>.ts.net:8443/mcp`.
- `--bg` configurations persist across reboots; `tailscaled` holds them.
- The daemon only ever binds loopback, so there is no boot-time race with
  Tailscale coming up: its loopback listeners answer any `*.ts.net` Host, the
  name Serve forwards, even if Tailscale was down when the daemon started.
- The HTTPS certificate is issued for the Mac's MagicDNS name, and issued
  certificates are recorded in public Certificate Transparency logs: the
  machine and tailnet names become public (nothing else does).
- To remove one mapping, run `tailscale serve --https=8443 off`.
  `tailscale serve reset` clears **all** Serve config on the machine.
- For requests from user-owned devices, Serve adds `Tailscale-User-Login` and
  `Tailscale-User-Name` headers. With `FSONOS_TAILSCALE_SERVE` set, the HTTP
  API takes `Tailscale-User-Login` on its loopback listeners as the caller:
  a `[clients."<login>"]` table in `policy.toml` applies to that person, and
  the action log names them. Requests from tagged devices carry no login and
  stay `loopback-http`. Any process on the Mac that can reach the loopback
  port could send the header too, so grant a login no more than you trust
  the Mac's own processes with. MCP over Serve is still `loopback-http`.

### Alternative: listen on the tailnet directly

Leave `FSONOS_HTTP_ADDR` unset and `fsonos serve` listens on loopback **and**
on every tailnet address of the Mac whenever Tailscale is up at startup; the
startup log prints the URLs (MagicDNS first) for tailnet devices. Traffic is
plain HTTP inside WireGuard. Two things to know:

- Callers on a direct tailnet listener are identified as `unknown`, which the
  default house policy keeps read-only until tailnet identity reaches the
  HTTP layer. Serve traffic arrives on loopback and has full control.
- The tailnet addresses are read once, at startup. If the daemon starts
  before Tailscale is up, it listens on loopback only and logs why; restart it
  (`launchctl kickstart -k`) once Tailscale is up. Serve avoids this.
  `fsonos doctor --only tailscale` checks the whole chain: Tailscale up and
  logged in, the MagicDNS name, and the daemon answering over the tailnet,
  with the connect URLs or the fix.

The MCP server stays on loopback unless `FSONOS_MCP_HTTP_ADDR` is set: one MCP
backend serves every MCP listener with a single caller identity, so a direct
tailnet MCP listener would share loopback's rights. Use Serve for MCP.

### Restrict who can reach it (tailnet policy)

A new tailnet's default policy lets all of your devices reach each other. To
limit the daemon to specific people or agent machines, merge grants like these
into your policy file (HuJSON). Note that grants only **add** access: while
the default allow-all rule is still present, these restrict nothing. Narrow
that rule first, and keep whatever other access you rely on, such as SSH to
the Mac.

```jsonc
{
  "tagOwners": { "tag:agent": ["autogroup:admin"] },
  // An alias for the Mac; replace with its `tailscale ip -4`. A host alias,
  // not a tag, so the Mac keeps belonging to you.
  "hosts": { "fsonos-host": "100.101.102.103" },
  "grants": [
    // You: the API and MCP from any of your devices.
    { "src": ["you@example.com"], "dst": ["fsonos-host"], "ip": ["tcp:443", "tcp:8443"] },
    // Agent machines tagged tag:agent: MCP only.
    { "src": ["tag:agent"],       "dst": ["fsonos-host"], "ip": ["tcp:8443"] }
  ]
}
```

If you bind the tailnet address directly, grant `tcp:8099` and `tcp:8098`
instead.

### Point your agents at it

```bash
# Claude Code, any tailnet machine (streamable HTTP):
claude mcp add --transport http fsonos https://<mac>.<tailnet>.ts.net:8443/mcp

# A local agent on the Mac itself, over HTTP (shares the daemon's live state):
claude mcp add --transport http fsonos http://127.0.0.1:8098/mcp

# A local agent that only speaks stdio:
claude mcp add fsonos -- ~/.local/bin/fsonos mcp
```

The MCP endpoint speaks the current MCP protocol era (`2026-07-28`); a client
that only speaks an older streamable-HTTP revision may be refused there, in
which case use `fsonos mcp` over stdio on the Mac. Other MCP clients take the
same URL, typically as
`{"mcpServers": {"fsonos": {"type": "http", "url": "https://<mac>.<tailnet>.ts.net:8443/mcp"}}}`.
Plain HTTP clients use the API directly, e.g.
`curl https://<mac>.<tailnet>.ts.net/zones`.

### Spotify sign-in (one time, for browsing and the DJ)

```text
local browser → SSH tunnel → daemon login → Spotify consent
                                      callback → private token cache
app or curl → POST /spotify/sync → cached library → browse and DJ
```

1. Create an app in the Spotify developer dashboard and copy its client id.
2. Register `http://127.0.0.1:8099/auth/spotify/callback` as its redirect URI.
3. Set `FSONOS_SPOTIFY_CLIENT_ID` in the daemon's environment.
4. Start the daemon with the loopback HTTP listener on port 8099.
5. Open an SSH tunnel from your browser's machine:

   ```bash
   ssh -L 8099:127.0.0.1:8099 <daemon-host>
   ```

   The forwarding form is `ssh -L 8099:<daemon-host>:8099 <ssh-host>`.
   Use `127.0.0.1` as the destination when SSH terminates on the daemon host,
   so the daemon identifies the forwarded connection as loopback.

6. Open `http://127.0.0.1:8099/auth/spotify/login` in the local browser.
7. Approve Spotify's read-only library permission.
8. Start the library sync:

   ```bash
   curl -X POST -H 'Content-Type: application/json' \
     http://127.0.0.1:8099/spotify/sync
   ```

9. Poll `http://127.0.0.1:8099/spotify/status` until `sync.running` is false.

The callback says "Signed in. You can close this tab." The pending sign-in
expires after ten minutes and is single use. Spotify accepts plain HTTP for
loopback IP literals, including `127.0.0.1`, rather than `localhost`. If you
change `FSONOS_SPOTIFY_REDIRECT_URI`, register that exact URI and tunnel its port.
Tokens stay in `FSONOS_DATA_DIR/auth/spotify-token.json`, written owner-only.
Artwork URLs are cached as metadata; the daemon does not fetch the images.

`POST /play` plays an album in the selected room's group:

```bash
curl -X POST -H 'Content-Type: application/json' \
  http://127.0.0.1:8099/play \
  -d '{"zone":"<room>","source_uri":"spotify:album:<album-id>"}'
```

An `open.spotify.com/album/<album-id>` link works too. The daemon replaces the
group coordinator's queue with the album's tracks and starts at track 1.
Synced albums play from the cache without a current daemon sign-in; an
uncached album requires the stored Spotify token. The daemon queues the first
100 tracks of a larger album and states the cap in its response. Cached albums
use their stored title; for a fetched album, supply `title` for the success
message, or it uses the album URI. Each household still needs a Spotify track
in My Sonos to learn its render settings.

Album playback requires the existing `play` policy permission. Playlists,
artists, shows and episodes remain unsupported. A queue-write failure reports
the confirmed track count; inspect the queue before retrying. See
[Spotify album playback errors](ERRORS.md#spotify-album-playback).

LAN and direct tailnet callers are `unknown`. Their default policy allows
reads; `spotify_sync` requires an explicit allow rule. If `policy.toml` already
has `[clients.unknown] allow`, include all of the new browsing operations you
want clients to call, preserving its existing operation ids:

```toml
[clients.unknown]
allow = ["spotify_status", "spotify_sync", "list_spotify_albums",
         "list_spotify_album_tracks", "list_spotify_tracks"]
```

Restart the daemon after changing its policy or environment. The login and
callback operations are `spotify_login` and `spotify_callback`; they always
require a loopback caller, even when a policy allow list includes them.

`sync.done` and `sync.total` count received library entries (saved albums,
liked tracks, and additional album tracks). The total grows as pages reveal
more work. A 429 keeps the worker running and reports `sync.error` with
`retryable:true` and `retry_at` in Unix seconds while honoring `Retry-After`.
A revoked refresh token sets `reauthorize:true`; sign in again before syncing.
`library.tracks` counts liked tracks, matching `/spotify/tracks`.

## 6. Verify and troubleshoot

```bash
sudo launchctl print system/$LABEL | grep -E 'state|last exit'   # want: state = running
grep 'fsonos serve: ready' ~/Library/Logs/fsonos/fsonos.log       # the bound addresses
grep 'fsonos serve: live' ~/Library/Logs/fsonos/fsonos.log        # households found, events address
curl -fsS http://127.0.0.1:8099/health                           # on the Mac
curl -fsS http://127.0.0.1:8099/zones                            # rooms and what they play
curl -fsS http://127.0.0.1:8099/openapi.json                     # every route, body and error code
curl -NsS http://127.0.0.1:8099/events                           # live changes (server-sent events)
tailscale serve status
curl -fsS https://<mac>.<tailnet>.ts.net/health                  # from another tailnet device
```

| Symptom | Likely cause |
|---|---|
| Discovery finds no players under a LaunchAgent; connects fail with "No route to host" | Local Network access denied or never approved. Approve it in System Settings, or switch to the LaunchDaemon. |
| Every call answers `NOT_READY` ("no rooms discovered yet") | The daemon found no players: SSDP is filtered on this network (set `FSONOS_SEEDS`), or Local Network access is missing (above). It keeps retrying; no restart needed. |
| Players found, but state never updates after changes made in the Sonos app | Event callbacks are blocked inbound: `GET /doctor` shows `daemon.live` with no event subscriptions. Check the Application Firewall and the events port (§4). |
| The log shows bind failures ("Can't assign requested address") right after boot | A tailnet address set in `FSONOS_HTTP_ADDR`, bound before Tailscale was up. It self-heals via `KeepAlive`; prefer Serve, or leave the address unset. |
| `launchctl bootstrap` fails with an I/O or permission error | Plist not `root:wheel` `0644`, or the job is already loaded. Run `bootout` first. |
| Tailnet clients time out but loopback works | Run `fsonos doctor --only tailscale` on the Mac: it says whether Tailscale is up and whether the daemon listens on the tailnet. If both pass, the tailnet policy doesn't grant the port, or Serve isn't configured (`tailscale serve status`). |

### Phone Spotify sign-in and speaker artwork

Register `frankensonos://spotify-callback` alongside the loopback callback in
Spotify's dashboard. `FSONOS_SPOTIFY_APP_REDIRECT_URI` (or
`--spotify-app-redirect-uri`) changes the app callback. `GET /spotify/status`
returns `client_id` when configured and `app_redirect_uri`. The app completes
PKCE consent and posts `code`, `code_verifier` and the exact `redirect_uri` to
`POST /auth/spotify/exchange`. Tokens stay on the daemon. A successful exchange
clears the previous account's library and browse cache; run `POST /spotify/sync`
for the new account.

Add `spotify_exchange` and `get_art` to the existing NAS caller's allow list,
preserving the status, sync, browse and playback ids already allowed. Exchange
is available to policy-approved LAN and tailnet callers. The existing login
and callback routes still require loopback. `GET /art` accepts a discovered
player id and its `/getaa` path; the daemon fetches only that player's port 1400.
