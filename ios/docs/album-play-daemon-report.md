# Album playback daemon report

Album playback is implemented. Socket tests compile, but their runtime red/green
results and real Sonos playback are unverified. The sandbox cannot open loopback
sockets, so no socket test was executed.

## Behavior

`POST /play` accepts `spotify:album:<id>` and the existing canonicalized
`open.spotify.com/album` links. The operation id remains `play`. Authorization
runs before planning, Spotify reads, or queue changes.

```text
play policy -> coordinator -> cached album or existing Spotify fetch
                                      |
                         learn render settings from favorites
                                      |
                         clear -> add in order -> queue track 1 -> play
                                      |
                       failure: stop and report confirmed additions
```

The daemon reads the same saved-album membership and library rows used by
`list_spotify_album_tracks`, ordered by disc and track number. It queues at most
100 tracks. A cached album uses its stored title and can play without a current
Spotify sign-in. Uncached albums use the existing
`fsonos_spotify::expand::fetch_album_tracks` with the stored token. No Spotify
endpoint wrapper or dependency was added. The existing client filters tracks
unavailable in the account's market. For fetched albums, `title` supplies the
response label; otherwise the album URI supplies it.

Render settings use `control::spotify_params` and `spotify_track_didl`, the
same primitives called by `control::spotify_track_source`. Learning happens
once per album. The HTTP test compares each album track's DIDL to the actual
single-track HTTP response's SOAP metadata.

Playlist, artist, show and episode requests remain `501 NOT_IMPLEMENTED`.
Their hint names tracks and albums. A missing configuration on `/play` returns
409 with the existing `SPOTIFY_NOT_CONFIGURED` code and configuration hint.
Other Spotify configuration routes retain their existing 503 response.

## Partial-write decision (P4)

The implementation reports partial additions rather than restoring the old
queue. Preflight completes before clearing. If clearing, adding a track, or
starting playback fails, the response is `502 UPNP_FAULT`, `retryable: false`,
with the confirmed count and the failed step. For example:

```text
album playback failed while adding a track:
1 of 3 tracks added (confirmed); soap fault 701: ...
```

The coordinator's old queue has been replaced once clearing succeeds. The
operation stops at the failure and bypasses automatic command healing. For a
lost or malformed reply, the failed request may also have applied. The hint
instructs the caller to inspect the queue before retrying. The daemon does
not claim partial success or restoration. `docs/ERRORS.md` documents this choice.

## Files changed

| File | Change |
|---|---|
| `crates/fsonos-api/src/execute.rs` | Album queue execution, partial-failure reporting, supported-kind hint; existing P6 regression expectation |
| `crates/fsonos-api/src/surface.rs` | Dispatch planned album commands after authorization, outside single-command retry healing |
| `crates/fsonos-api/src/spotify.rs` | Resolve cached album tracks and reuse the existing authenticated album fetch |
| `crates/fsonos-api/src/http.rs` | `/play` configuration failure returns 409 with the existing code/hint |
| `crates/fsonos-api/tests/album_play.rs` | P1–P9 HTTP/simulator scenarios and uncached paged-fetch success |
| `crates/fsonos-api/tests/support/spotify.rs` | Reuse the existing HTTP/fake-Spotify harness with a simulator-backed Surface |
| `crates/fsonos-cli/tests/e2e_spotify.rs` | Real daemon sign-in, sync and album tap; queue/playback checks and scenario artifact |
| `docs/ERRORS.md` | Album preflight, errors and partial queue behavior |
| `docs/DEPLOY.md` | Album request, sign-in/cache requirements, cap and existing `play` permission |
| `ios/docs/album-play-daemon-report.md` | This requested report, the sole `ios/` write |

No staging, commits, deletions or deployment occurred. The existing branch is
`ios-app`; it was not changed. The pre-existing untracked
`docs/spotify-daemon-worklog.md` was preserved. `.github/` and iOS application
sources were untouched. The task worklog is `/private/tmp/album-play-worklog.md`.

## Tests-first evidence

All P1–P9 socket scenarios and the CLI album scenario were written before the
production album implementation. API and CLI baseline `--no-run` checks passed
before production changes. An initial API compile attempt exposed two test
setup type errors (`DidlRes` versus `String`, and `String` versus `PlayerId`);
they were corrected before the successful baseline compile. Those compiler
errors are not feature-regression evidence.

The socket-free P6 regression was run on the baseline before implementation.
Command and actual red output:

```text
cargo test -p fsonos-api --lib \
  unwired_paths_say_so_without_touching_speakers -- --nocapture

Play one of its tracks (spotify:track:...) for now.
test execute::tests::unwired_paths_say_so_without_touching_speakers ... FAILED
test result: FAILED. 0 passed; 1 failed; 79 filtered out
```

The same test passed after implementation. Full red and green output is in
`/private/tmp/album-red-hint.log` and `/private/tmp/album-green-hint.log`.
The failure was the hint omitting album support.

No socket scenario has observed runtime red or green evidence. Compilation
is the evidence available for those rows. P7 protects existing authorization
behavior; it is not a claim that the baseline denied policy incorrectly.

## Evidence by failure-matrix row

All named HTTP tests below are in `crates/fsonos-api/tests/album_play.rs`.

| Row | Test and assertions | Red evidence | Green evidence |
|---|---|---|---|
| P1 | `p1_cached_album_clears_orders_and_plays_from_one_with_track_didl`: old queue replaced, reversed cache rows queued in order, track 1 playing, stored album title/count, DIDL equals single-track path | Written first; socket runtime not run | Final compile passed; runtime unverified |
| P2 | `p2_uncached_signed_out_or_unconfigured_preserves_queue`: configured/unconfigured cases return 4xx and existing Spotify codes/hints; seeded old queue remains | Written first; socket runtime not run | Final compile passed; runtime unverified |
| P3 | `p3_unknown_album_fetch_fails_before_clearing`: authenticated upstream 404 produces sync-or-track error and preserves old queue | Written first; socket runtime not run | Final compile passed; runtime unverified |
| P4 | `p4_nth_enqueue_failure_reports_confirmed_partial_count`: second add refuses, 502/nonretryable, one confirmed track remains, no Play | Written first; socket runtime not run | Final compile passed; runtime unverified |
| P5 | `p5_missing_favorite_preserves_queue_with_track_error`: household favorites removed, same render-params code and My Sonos hint, old queue remains | Written first; socket runtime not run | Final compile passed; runtime unverified |
| P6 | `p6_unsupported_kinds_name_track_and_album`: all four unsupported kinds return NOT_IMPLEMENTED and track/album hint, queue unchanged | Socket runtime not run; existing pure hint regression observed red | Pure hint regression passed; HTTP scenario compiled only |
| P7 | `p7_play_policy_denies_before_queue_changes`: caller without play permission denied before queue change or Spotify request | Written first; existing policy is already a guard; no socket runtime red | Final compile passed; runtime unverified |
| P8 | `p8_large_album_caps_and_reports_first_hundred`: 101 cached tracks yield first 100 in disc order with explicit truncation summary | Written first; socket runtime not run | Final compile passed; runtime unverified |
| P9 | `p9_grouped_room_builds_coordinator_queue_and_canonicalizes_link`: Bedroom grouped under Living Room; album link sent for Bedroom builds Living Room queue | Written first; socket runtime not run | Final compile passed; runtime unverified |

Additional tests written before implementation:

| Test | Coverage | Evidence |
|---|---|---|
| API `uncached_album_uses_existing_paged_spotify_client_before_queueing` | Authenticated uncached album, multiple Spotify pages, six tracks, queue playback and secret checks | Baseline/final compile passed; runtime unverified |
| CLI `album_tap_after_sync_plays_the_whole_album` | Launch daemon against simulator/fake Spotify, sign in, sync, submit album web link, verify cached title, three queue tracks and playback from track 1 | Baseline/final compile passed; runtime unverified |

## Checks executed

| Check | Result | Log |
|---|---|---|
| `cargo test -p fsonos-api --no-run` before implementation | Passed after test setup corrections | `/private/tmp/album-baseline-api-compile.log` |
| `cargo test -p fsonos-cli --no-run` before implementation | Passed | `/private/tmp/album-red-cli-compile.log` |
| `cargo fmt` | Passed | Ran directly |
| `cargo fmt --check` | Passed | Ran directly |
| `cargo check --workspace --all-targets` | Passed | `/private/tmp/album-check.log` |
| `cargo clippy --workspace --all-targets` | Passed with one test-length warning, subsequently fixed | `/private/tmp/album-clippy.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed without warnings | `/private/tmp/album-clippy-final.log` |
| Final `cargo test -p fsonos-api --no-run` | Passed, including all new API scenarios | `/private/tmp/album-final-api-compile.log` |
| Final `cargo test -p fsonos-cli --no-run` | Passed, including the real-daemon album scenario | `/private/tmp/album-green-cli-compile.log` |
| Socket-free workspace tests | 494 passed, 0 failed, 26 filtered across 25 suites | `/private/tmp/album-green-pure.log` |
| `cargo test -p fsonos-api --lib errors_doc_matches_the_codes` | Passed against updated docs | `/private/tmp/album-errors-doc.log` |
| `cargo test --workspace --doc` | Passed; one simulator `no_run` example compiled, no socket execution | `/private/tmp/album-doc-tests.log` |
| `git diff --check` | Passed | Ran directly |

The socket-free workspace command was:

```bash
cargo test --workspace --lib --bins \
  --test control --test events --test favorites --test fsqlite_roundtrip \
  --test playback --test reconcile --test rooms_resolution --test search \
  --test store_conformance --test survey --test topology_golden \
  --test spotify_contract --test golden --test status_golden \
  --test exit_codes --test mcp_stdio -- \
  --skip net::tests:: --skip feed::tests:: --skip session::tests:: \
  --skip sync_library_end_to_end_against_fake_spotify \
  --skip albums_are_read_once_through_429_and_paging_then_cached \
  --skip a_rate_limit_stops_the_run_and_unplayable_albums_wait \
  --skip a_missing_daemon_is_a_warning_with_a_remedy \
  --skip routes_and_banner_cover_every_player_on_loopback \
  --skip single_household_scenarios
```

## What remains unverified

The P1–P9 socket scenarios, paged-fetch scenario and CLI album scenario need to
run outside the socket-restricted sandbox. Existing API socket suites
(`spotify_daemon`, `spotify_exchange`, `health`, `http_api`, `art`, and simulator
suites), CLI E2E/simulator suites, protocol HTTP tests, core simulator/LAN
suites and the 26 excluded socket-dependent unit tests were not executed.
Full `cargo test --workspace` was not run because it includes those tests.
The existing live-LAN tests require the owner's opt-in environment.

The caller can run the album scenarios with:

```bash
cargo test -p fsonos-api --test album_play -- --nocapture
cargo test -p fsonos-cli --test e2e_spotify -- --nocapture
cargo test --workspace
```

The CLI scenario writes its normal `target/e2e-logs` artifact when executed.
Real-device rendering, mid-call coordinator changes and network failures with
ambiguous write acknowledgments have no runtime evidence from this session.
There is no claim of hardware or integration success from the compile checks.

`br` and `rch` are unavailable, so no bead was updated and builds ran locally.
GitHub read-only discovery could not reach `api.github.com`. Shared Mnemopi
recall and retention were rejected because those calls require approval while
this session's approval policy is never; no memory write was claimed.
