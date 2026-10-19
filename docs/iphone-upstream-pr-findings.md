# iPhone upstream PR: verification findings

Checked against `upstream/main` @ `bcd4208819d0d8e35768298a13fbb31d1b169f39`
(fetched shallow in this session). `origin/upstream-pr` @ `d22d3faf1e8fc84fbfeca7c2afb5507d10642337`
is on the fork. The fork's own `main` (`541f101`) is BEHIND upstream and has no `apps/`; work must be
based on `upstream/main`, not `origin/main`.

## 1. Custom-scheme redirect (`frankensonos://...`): does NOT work as-is

- `parse_callback` (`crates/fsonos-spotify/src/client.rs`) accepts any string: it takes what follows
  `?`, checks `state`, returns `code`. No scheme check. `complete_authorization` (session.rs) and
  `POST /auth/spotify/complete` (`{redirect}`) pass the string straight through. So the
  begin -> complete half would accept a custom-scheme landing URL.
- BUT `SpotifyConfig::validate()` (client.rs) requires the daemon's configured redirect URI to be
  `https://...` or `http://127.0.0.1` / `[::1]`. `SpotifySignIn::new` runs it and, on failure, logs
  "Spotify sign-in off" and installs no sign-in engine. So `FSONOS_SPOTIFY_REDIRECT_URI=frankensonos://...`
  turns sign-in off entirely.
- The redirect URI is ONE daemon-wide value (`FSONOS_SPOTIFY_REDIRECT_URI`), used in both the authorize
  URL and the token exchange. The app cannot choose its own per call.
- Upstream's Swift `SpotifyAddress.redirect` also requires https or loopback http.

### What does work with zero daemon change

Set `FSONOS_SPOTIFY_REDIRECT_URI=https://<daemon-tailnet-name>/auth/spotify/callback` (and register it in
the Spotify dashboard). The phone is on the tailnet, so its browser reaches that route; the daemon's own
`GET /auth/spotify/callback` completes the sign-in inside the daemon. The verifier and `state` never
leave the daemon, and the app never sees the code. The app only does:

1. `POST /auth/spotify/begin` -> `{url, redirect_uri}`; validate url (host accounts.spotify.com, S256,
   exact redirect_uri), as upstream's `SpotifySetup.swift` already does.
2. Open `url` in Safari / `SFSafariViewController`.
3. Poll `GET /auth/spotify` until `signed_in`.

No `/complete` needed, no custom scheme, no new route. `/complete` stays available as the paste-back
fallback. Cost: the owner must run the daemon with a tailnet HTTPS redirect URI (a deployment setting,
not a code change) instead of the loopback default.

## 2. `/library/search` vs `/library/browse`

Both read the SAME local cache (filled by `POST /dj/sync`); neither reaches Spotify's catalog.

| | `/library/browse` | `/library/search` |
|---|---|---|
| Source | cached tracks / albums / playlists | cached tracks, saved albums, playlists, plus the room's household Sonos favorites when `zone=` is given |
| Paging | `offset`, `limit` 1-50, `next_offset`, `total` | `limit` 1-50 only, scored, no paging |
| Query | `q`, every word a case-folded substring of title+artist+album | scored (`search::search_all`) |
| Result | `BrowseItem` (source_uri, title, subtitle, album, track no., duration, explicit) | `HitDto` (kind track/album/playlist/favorite, source_uri or favorite id, score) |
| Favorites | no | yes |

So the Search tab should use `/library/search?q=&zone=` (it is the only route that finds Sonos
favorites and it ranks); library lists use `/library/browse` (the only paged one). Neither finds a track
the owner has not saved. Catalog search is therefore PR B (`GET /spotify/search`), as planned.

## 3. Art

`GET /art?zone=` serves only the now-playing art of a room's group, and refuses `url=`/`uri=` (test
`art_takes_a_room_never_a_url`). It runs `same_origin`, which allows a missing `Sec-Fetch-Site`, so a
native client is fine. `BrowseItem` and `HitDto` carry NO image field, so browse and search rows cannot
have real art through main's API. Rows get generated placeholder art (our `SketchRenderer`) until the
daemon exposes art for library items, which would be its own decision.

## 4. Upstream `apps/apple` gaps (our features that remain)

Grep of `upstream/main:apps/apple` finds NO: AppIntent/Shortcuts, `/library/browse`, `/library/search`,
`/art`, Spotify link paste, `open.spotify` handling, ASWebAuthenticationSession/openURL, or any
iOS-only UI (`#if os(iOS)`). `RemoteApp.swift` is macOS-only. Present and reusable in `FrankenSonosKit`:
daemon client with HTTPS-only endpoint, house snapshot/live events, commands with review, scenes,
schedules, sleep, activity replay, Spotify setup with consent-URL validation.

## 5. Constraint on this container

There is no Swift toolchain here (`swift`, `xcodebuild` absent; Linux x86_64), so the iPhone UI target
cannot be compiled or its XCTests run in this session. Cargo exists, so any daemon work (PR B) can be
built and tested here. CI for the Swift package is `.github/workflows/apple-remote.yml` (macOS).

## 6. Not carried over from `upstream-pr`

Daemon-side: `crates/fsonos-api/src/spotify.rs`, route additions in `http.rs`, LAN bind list in
`fsonos-cli/src/config.rs`, `daemon.rs`, `deploy/docker/Dockerfile`, `docs/DEPLOY.md`.
Also dropped: the `ios/` tree as a separate app (its views, intents, `SketchRenderer`, and stub-daemon
get ported into `apps/apple`).
