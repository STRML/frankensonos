# Spotify sign-in from the app

The daemon-hosted flow in `spotify-plan.md` needs a browser that reaches the daemon's loopback address, so it only
works from a Mac. A phone's browser cannot reach the NAS's `127.0.0.1`, and Spotify only accepts a plain `http`
redirect to a loopback address. Apps use a custom URL scheme instead.

```
app                                                   Spotify                       daemon (holds the tokens)
 │ GET /spotify/status ───────────────────────────────────────────────────────────▶ client_id, app_redirect_uri
 │ make verifier + challenge + state
 │ ASWebAuthenticationSession ──authorize?client_id&code_challenge&redirect_uri=frankensonos://spotify-callback
 │ ◀── frankensonos://spotify-callback?code=…&state=… (the system sheet hands it to the app)
 │ check state
 │ POST /auth/spotify/exchange {code, code_verifier, redirect_uri} ───────────────▶ code → tokens (token endpoint),
 │                                                                                  cache written, signed in
 │ POST /spotify/sync ─────────────────────────────────────────────────────────────▶ library into the cache
```

The Spotify app needs `frankensonos://spotify-callback` in its redirect URIs. The loopback URI for the Mac flow can
stay registered next to it.

## Daemon contract (new and changed)

| Route | Operation id | Behavior |
|---|---|---|
| `GET /spotify/status` | `spotify_status` | adds `client_id` (string, only when configured) and `app_redirect_uri` (string) |
| `POST /auth/spotify/exchange` | `spotify_exchange` | body `{code, code_verifier, redirect_uri}`; any caller the policy allows (not loopback-only); finishes a sign-in the app started; answers `{"signed_in": true}` |

`app_redirect_uri` comes from `FSONOS_SPOTIFY_APP_REDIRECT_URI`, default `frankensonos://spotify-callback`. The
exchange accepts exactly that value and nothing else. The existing login and callback routes keep working.

## Failure matrix (daemon)

| # | State or input | What the daemon does | What the caller is told |
|---|---|---|---|
| E1 | Not configured | nothing | 503 `SPOTIFY_NOT_CONFIGURED` with the hint |
| E2 | `redirect_uri` is not the configured app redirect | nothing sent to Spotify | 400 `SPOTIFY_REDIRECT_NOT_ALLOWED` |
| E3 | `code_verifier` not 43 to 128 characters of `A-Z a-z 0-9 - . _ ~`, or `code` empty or over 512 | nothing sent | 400 `INVALID_ARGUMENT` |
| E4 | Spotify rejects the code (`invalid_grant`, wrong verifier, expired) | no token stored, still signed out | 400 with the safe Spotify error text, never an upstream description that could echo a secret |
| E5 | Spotify unreachable or 5xx | no token stored | 502 with `retryable: true` |
| E6 | A sign-in, exchange or sync already running | nothing | 409 busy |
| E7 | Token cache cannot be written | status stays signed out | the cache path in the error |
| E8 | Success while already signed in | replaces the stored token (the owner switching accounts) | `{"signed_in": true}`; the library cache is cleared so the next sync reads the new account |
| E9 | Secrets | the code, verifier and tokens never appear in a response, a log line or the action log | tests grep all three |
| E10 | Policy | `spotify_exchange` is an ordinary operation id, so a caller needs it in its allow list | `POLICY_DENIED` names it |

## Failure matrix (app)

| # | State | What the app does |
|---|---|---|
| S1 | Daemon not configured or unreachable | no sign-in button; the existing explanation and steps |
| S2 | User cancels the sheet | back to the sign-in screen quietly, no error |
| S3 | Spotify returns `error=access_denied` or any `error` | shows it, sends nothing to the daemon |
| S4 | Callback `state` differs from the one sent | rejected, nothing sent to the daemon |
| S5 | Exchange refused by the daemon | shows the daemon's text and hint |
| S6 | Success | status refreshed, a library sync starts, progress shows |
| S7 | Already signed in | Settings of the Spotify screen offers "Switch account", which runs the same flow |
| S8 | The system sheet cannot start | the error, and "Try again" |

## Tests

PKCE against the RFC 7636 appendix B vector (verifier `dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk`, challenge
`E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM`), authorize URL contents, callback parsing (code, error, state
mismatch, missing code) as pure tests. Daemon E1 to E10 as integration tests against `fake_spotify`. App E2E against the
stub daemon: status carries the client id, the exchange body is exactly what the plan says, and the sign-in service
completes with a canned callback. Only the system sheet itself needs a device.
