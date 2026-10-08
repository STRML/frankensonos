# Spotify: sign in on the daemon, browse and play from the app

What exists, read from the code:

| Piece | State |
|---|---|
| PKCE, authorize URL, code exchange, token cache, refresh, paged library reads (`fsonos-spotify`: `client.rs`, `session.rs`, `cache.rs`) | done, unit tested, never used by the daemon |
| Library cache in the store, `pool_from_store`, the classical DJ (`fsonos-cli/src/dj.rs`, installed in `serve`) | done; the DJ answers `NO_DJ_SESSION` ("Sign in to Spotify on the daemon host and sync the library") because the cache is empty |
| `POST /play` with `spotify:` URIs or `open.spotify.com` links, `GET /library/search` (cache plus Sonos favorites), `/dj/start|skip|stop` | done |
| Login route, callback route, sync trigger, library listing for a client | missing: this plan |

```
browser on a Mac ──ssh -L 8099:...──▶ GET /auth/spotify/login ─302▶ accounts.spotify.com
                                      GET /auth/spotify/callback?code ─▶ token cache (data dir)
app ──▶ GET /spotify/status  ──▶ POST /spotify/sync (202) ──▶ GET /spotify/albums, /spotify/tracks
app ──▶ POST /play {zone, source_uri: spotify:album:...}      GET /library/search?q=
app ──▶ POST /dj/start {zone}                                 (works once the cache is filled)
```

## Daemon API (new)

| Route | Operation id | Behavior |
|---|---|---|
| `GET /auth/spotify/login` | `spotify_login` | loopback callers only. 302 to the authorize URL; keeps the PKCE verifier and state in memory for 10 minutes. `503 SPOTIFY_NOT_CONFIGURED` with a hint when `FSONOS_SPOTIFY_CLIENT_ID` is unset |
| `GET /auth/spotify/callback` | `spotify_callback` | loopback only. Checks state, exchanges the code, writes the token cache, answers a small HTML page ("Signed in. You can close this tab.") |
| `GET /spotify/status` | `spotify_status` | `{configured, signed_in, reauthorize, library: {albums, tracks, synced_at}, sync: {running, done, total, error}}` |
| `POST /spotify/sync` | `spotify_sync` | starts a background library read into the store cache, returns `202` at once, single flight |
| `GET /spotify/albums?offset&limit&q` | `list_spotify_albums` | `{total, items: [{id, title, artist, year, tracks, uri, art_url}]}` from the cache |
| `GET /spotify/albums/{id}/tracks` | `list_spotify_album_tracks` | `[{id, title, artists, uri, duration_secs, disc, number}]` |
| `GET /spotify/tracks?offset&limit&q` | `list_spotify_tracks` | liked tracks, same shape as album tracks plus `album` and `art_url` |

`art_url` is the image URL Spotify returns (a public CDN address); the daemon stores it, never fetches it. Tokens
never appear in a response or a log line.

## Failure matrix (daemon)

| # | State or input | What the daemon does | How it can fail | What the caller is told |
|---|---|---|---|---|
| D1 | No client id configured | login 503, status `configured:false` | env var missing | hint: set `FSONOS_SPOTIFY_CLIENT_ID`, register the redirect URI |
| D2 | Login or callback from a non-loopback address | refuse | someone on the LAN starts or finishes an auth | 403 `FORBIDDEN_NOT_LOOPBACK` |
| D3 | Callback with a wrong, missing or replayed state | discard the pending authorization, store nothing | CSRF, double click | 400 page, no token written |
| D4 | Code exchange fails (`invalid_grant`, network, 5xx) | store nothing | Spotify down, code expired | page shows Spotify's error text |
| D5 | Token cache cannot be written | status `signed_in:false` | data dir not writable | clear error naming the path |
| D6 | Refresh token revoked or expired | `reauthorize:true`, sync stops, no retry loop | user removed the app in Spotify | status says sign in again |
| D7 | Sync requested while one runs | single flight | double tap | `202` with the running progress |
| D8 | Spotify answers 429 | honor `Retry-After`, keep progress | big library | status `error` with `retryable` and a retry time |
| D9 | Library of thousands of items | paged, in the background, progress in status | request timeout if done inline | `202`, then poll status |
| D10 | Listing before any sync | empty `items`, `synced_at:null` | none | not an error |
| D11 | Policy | new operation ids need the allow list | LAN callers are `unknown` and read-only by default | `POLICY_DENIED` names the operation |
| D12 | Secrets | never in bodies, logs or the action log | a debug line prints a token | tests grep both for the fake's token strings |

## Failure matrix (app)

| # | State | What the app does |
|---|---|---|
| A1 | Not configured or not signed in | the Spotify screen shows the state and the exact steps, not an error |
| A2 | Sync running | progress line, lists fill in as the cache grows |
| A3 | Signed in, empty library | empty state with a Sync button |
| A4 | Tap an album or track | `POST /play` to the selected room's zone with `source_uri` and `title`; daemon failure text goes to the toast (for example Spotify not linked in Sonos) |
| A5 | Search box | debounce 300 ms, `GET /library/search`, last query wins |
| A6 | Artwork | `art_url` through the image cache; a failed load keeps the generated cover |
| A7 | DJ | start, skip and stop on the selected zone; `NO_DJ_SESSION` shows its hint and a Sync button |
| A8 | Policy denial of a Spotify route | the same "Live updates are off" style note, naming the operation, never a silent empty list |

## Test plan

Daemon, in Rust, against `fake_spotify.rs` (a local stand-in for the accounts and API hosts, endpoints overridable
for tests only): D1 to D10 and D12 as integration tests that start the real routes on loopback. Sandbox note: the
tests need loopback sockets, which the Codex sandbox forbids, so the orchestrator runs them.
App: E2E rows against the same daemon with the fake Spotify: status, sync to completion, list albums, play (the sim
speaker's transport state changes), DJ start answers.
