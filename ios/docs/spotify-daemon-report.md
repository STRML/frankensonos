# Spotify daemon implementation report

Implemented on `ios-app`. The daemon constructs Spotify sessions, serves the
seven planned routes, and syncs into the same store the existing DJ reads on
each start. No restart is required between successful sign-in, sync and DJ
start in the implemented path. That runtime sequence still needs the socket
E2E run below.

Nothing was staged or committed by this agent. No files were deleted. This
report is the only file this agent wrote under `ios/`. Existing route handlers
were preserved. Networking uses asupersync; routes use the existing builder
registration and policy gate. Crate roots retain `forbid(unsafe_code)`.

## Implementation

- Authorization keeps one pending PKCE flow with a ten-minute validity limit.
  A callback consumes it before validation or exchange, including invalid
  callbacks. The TTL is checked against `Instant`, not wall-clock time.
- Login and callback require `Client::LoopbackHttp`, using the existing caller
  identity. Identified Tailscale Serve users and unknown callers get
  `403 FORBIDDEN_NOT_LOOPBACK`, even if their policy allows the operation.
- Background sync has one worker across all listeners. A concurrent request
  receives `202` and the current progress. Network reads happen without holding
  the store lock. A complete read updates the DJ cache and browse metadata.
- Album artwork is Spotify's returned URL, stored as metadata without fetching
  the image. Browse membership is replaced on sync, so stale liked tracks do
  not remain in the listing after a later sync.
- Migration 7 adds the browse snapshot table. Previously applied migrations
  are unchanged. Existing database upgrade tests and reopen conformance passed.
- Rate limits expose a retryable error and Unix `retry_at`, preserving progress
  while the session waits. Exhausted retries retain the deadline; a new sync
  waits for that deadline. Revoked refresh stops sync and sets `reauthorize`.
- Tokens remain in the private token cache. DTOs and action entries do not
  contain them. Error pages preserve recognized Spotify OAuth error codes,
  escape HTML, and omit arbitrary upstream descriptions that could contain
  tokens. Cache I/O errors identify the cache path.
- `FSONOS_SPOTIFY_ACCOUNTS_URL` overrides the accounts base and
  `FSONOS_SPOTIFY_API_URL` overrides the API base. Both are documented for tests
  and fakes. Production defaults still use Spotify's hosts.

The browse snapshot becomes visible after a complete sync. While it runs,
clients see the previous completed snapshot and current progress. Progress
counts saved album entries, liked tracks and additional album tracks. The
advertised total can grow as pages reveal more work. `library.tracks` counts
liked tracks, matching `/spotify/tracks`.

## Files changed

| Files | Change |
|---|---|
| `Cargo.lock` | Internal API dependency on Spotify and the log-capture dev dependency; external versions unchanged |
| `crates/fsonos-api/Cargo.toml` | Spotify dependency, fake-host test support and tracing capture |
| `crates/fsonos-api/src/lib.rs` | Export the daemon Spotify module |
| `crates/fsonos-api/src/spotify.rs` | Shared authorization state, session workers, sync progress, DTOs, cache reads, policy admission and pure tests |
| `crates/fsonos-api/src/http.rs` | Seven builder routes, query/path schemas, operation ids and auth OpenAPI media/header details |
| `crates/fsonos-api/src/failure.rs` | `SPOTIFY_NOT_CONFIGURED` and `FORBIDDEN_NOT_LOOPBACK` |
| `crates/fsonos-api/src/surface.rs` | Attach Spotify and share the existing store through `Arc<Mutex<...>>` |
| `crates/fsonos-api/tests/spotify_daemon.rs` | Twelve real-route integration tests against fake Spotify |
| `crates/fsonos-api/tests/spotify_contract.rs` | Four pure route, policy, metadata and OpenAPI contract tests |
| `crates/fsonos-cli/Cargo.toml` | Fake Spotify dev feature for daemon E2E |
| `crates/fsonos-cli/src/config.rs` | Accounts/API override flags and environment variables |
| `crates/fsonos-cli/src/daemon.rs` | Construct and attach Spotify using the configured client id, redirect, data directory and endpoints |
| `crates/fsonos-cli/src/doctor.rs`, `crates/fsonos-cli/src/doctor/tailscale.rs` | Update existing configuration literals for the new fields |
| `crates/fsonos-cli/tests/e2e/mod.rs` | Remove ambient override environment variables from the E2E harness |
| `crates/fsonos-cli/tests/e2e_spotify.rs` | Real daemon sign-in, sync, DJ start, simulator playback and secret checks |
| `crates/fsonos-core/src/store.rs` | Browse metadata and minimal cache read/write Store methods, with MemStore support |
| `crates/fsonos-core/src/store/sqlite.rs` | Append migration 7 and persist the browse snapshot |
| `crates/fsonos-core/tests/store_conformance.rs` | Artwork/membership persistence and replacement across reopen |
| `crates/fsonos-spotify/Cargo.toml`, `crates/fsonos-spotify/src/lib.rs` | Expose the existing fake behind `test-support` |
| `crates/fsonos-spotify/src/client.rs` | Decode returned album image URLs |
| `crates/fsonos-spotify/src/library.rs` | Collect browse metadata, active membership and page progress alongside DJ library items |
| `crates/fsonos-spotify/src/session.rs` | Pure pending authorization construction and rate-limit observer; preserve existing session calls |
| `crates/fsonos-spotify/src/fake_spotify.rs` | Reusable fake, exchange failures, artwork and large paged library controls; unique scratch directories without deletion |
| `docs/DEPLOY.md` | Real dashboard/redirect/tunnel/sign-in/sync procedure, overrides and policy ids |
| `docs/ERRORS.md` | New stable error codes |
| `docs/spotify-daemon-worklog.md` | Goal, progress and verification limits |
| `ios/docs/spotify-daemon-report.md` | This report |

## Tests first and red evidence

The initial real-route integration suite was written before production wiring.
Its first compile was red. No socket test was executed for that red check:

```sh
CARGO_TARGET_DIR=/private/tmp/frankensonos-codex-target \
  cargo test -p fsonos-api --test spotify_daemon --no-run
```

Actual diagnostics from `/private/tmp/spotify-daemon-red.log`, exit 101:

```text
error[E0432]: unresolved import `fsonos_api::spotify`
error[E0432]: unresolved import `fsonos_spotify::fake_spotify`
error[E0599]: no method named `with_spotify` found for struct `Surface` in the current scope
Some errors have detailed explanations: E0432, E0599.
For more information about an error, try `rustc --explain E0432`.
error: could not compile `fsonos-api` (test "spotify_daemon") due to 3 previous errors
```

This establishes compile-red, not behavioral socket-red. Supplemental cases,
log capture and the CLI E2E were added during implementation.

A later pure OpenAPI assertion was added before correcting the auth response
schema. Its actual runtime-red tail was:

```text
assertion `left == right` failed
  left: Null
 right: "string"
failures:
    openapi_documents_all_spotify_operations_and_schemas
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.01s
error: test failed, to rerun pass `-p fsonos-api --test spotify_contract`
```

After documenting the Location header and HTML response types, the complete
pure contract suite passed:

```text
test cache_artwork_and_pagination_are_available_without_a_session ... ok
test openapi_documents_all_spotify_operations_and_schemas ... ok
test empty_cache_contract_and_configuration_hint ... ok
test unknown_auth_is_loopback_denial_and_policy_uses_operation_id ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

## Verification and gate tails

All cargo commands used `CARGO_TARGET_DIR=/private/tmp/frankensonos-codex-target`.
`rch` and `br` were unavailable, so the permitted gates ran locally. No beads
were claimed or closed. Shared-memory tools required approval unavailable in
this session; the worklog records progress locally.

| Check | Result |
|---|---|
| `cargo fmt --check` | Exit 0, no output |
| `cargo check --workspace --all-targets` | Exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo test --workspace --no-run` | Exit 0; all tests compiled, including both new socket suites |
| Corrected pure workspace run, command below | Exit 0; 489 passed, 0 failed, 26 filtered across 25 test binaries |
| Filtered `cargo test -p fsonos-spotify`, command below | Exit 0; 76 passed, 15 socket tests filtered |
| `cargo test --workspace --doc` | Exit 0; one `no_run` example compiled, no socket example executed |
| `cargo tree --workspace -d` | Exit 0; one asupersync 0.5.0 and fsqlite 0.4.9 graph |
| `git diff --check` | Exit 0 |

Actual check tail from `/private/tmp/spotify-daemon-check.log`:

```text
    Checking fsonos-api v0.1.0 (/Users/samuelreed/git/forks/frankensonos/crates/fsonos-api)
    Checking fsonos-mcp v0.1.0 (/Users/samuelreed/git/forks/frankensonos/crates/fsonos-mcp)
    Checking fsonos-cli v0.1.0 (/Users/samuelreed/git/forks/frankensonos/crates/fsonos-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.30s
```

Actual clippy tail from `/private/tmp/spotify-daemon-clippy.log`:

```text
    Checking fsonos-api v0.1.0 (/Users/samuelreed/git/forks/frankensonos/crates/fsonos-api)
    Checking fsonos-mcp v0.1.0 (/Users/samuelreed/git/forks/frankensonos/crates/fsonos-mcp)
    Checking fsonos-cli v0.1.0 (/Users/samuelreed/git/forks/frankensonos/crates/fsonos-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.38s
```

Actual compile-only output, selected lines from
`/private/tmp/spotify-daemon-no-run.log`:

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.47s
  Executable tests/spotify_contract.rs (/private/tmp/frankensonos-codex-target/debug/build/fsonos-api/b101e55318582561/out/spotify_contract-b101e55318582561)
  Executable tests/spotify_daemon.rs (/private/tmp/frankensonos-codex-target/debug/build/fsonos-api/a747489490860a3a/out/spotify_daemon-a747489490860a3a)
  Executable tests/e2e_spotify.rs (/private/tmp/frankensonos-codex-target/debug/build/fsonos-cli/5e9f21ad70182b6a/out/e2e_spotify-5e9f21ad70182b6a)
```

The corrected pure run included all library tests, pure CLI unit tests, the
Spotify contract suite, eleven pure core integration targets, protocol golden
fixtures, tailnet golden fixtures, CLI exit codes and MCP stdio:

```sh
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

The separately requested Spotify run excluded its existing socket tests:

```sh
cargo test -p fsonos-spotify -- \
  --skip feed::tests:: --skip session::tests:: \
  --skip sync_library_end_to_end_against_fake_spotify \
  --skip albums_are_read_once_through_429_and_paging_then_cached \
  --skip a_rate_limit_stops_the_run_and_unplayable_albums_wait
```

Actual Spotify result:

```text
test result: ok. 76 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 3.52s
```

The 26 filtered unit tests comprise eight protocol network tests, fifteen
Spotify socket tests, one CLI doctor socket test and two CLI simulator socket
tests. Socket integration targets were omitted from the run selection.

### Accidental simulator socket attempts

The first CLI unit-test filter missed two existing simulator tests. They
attempted socket creation and failed in the sandbox. A queued workspace run
repeated those two attempts before the exclusions were corrected. There were
two affected test names and four failed test invocations across those runs.
These failures are not counted as passes. The new Spotify socket suites were
never executed.

Actual tail from `/private/tmp/spotify-daemon-cli-tests.log`:

```text
Caused by:
    cannot start the simulator: Operation not permitted (os error 1)
failures:
    sim::tests::routes_and_banner_cover_every_player_on_loopback
    sim::tests::single_household_scenarios
test result: FAILED. 33 passed; 2 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.04s
error: test failed, to rerun pass `-p fsonos-cli --bin fsonos`
```

The corrected CLI unit result in the final workspace run was:

```text
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.03s
```

## D1 to D12 coverage

Socket tests below are in `crates/fsonos-api/tests/spotify_daemon.rs` unless
another file is named. Compiled means the test built successfully; it does not
mean its assertions passed at runtime.

| Row | Test coverage | Executed here |
|---|---|---|
| D1 | `d1_d10_unconfigured_and_empty_cache_are_explicit`: login 503, code/hint and configured false. Pure `spotify_contract::empty_cache_contract_and_configuration_hint` also checks the failure. | Socket compiled only; pure passed |
| D2 | `d2_auth_rejects_unknown_and_serve_callers_before_policy`: both auth routes, unknown and identified Serve callers, explicit allow list still denied. Pure `unknown_auth_is_loopback_denial_and_policy_uses_operation_id` checks the login denial. | Socket compiled only; pure passed |
| D3 | `d3_wrong_missing_and_replayed_state_store_nothing`: no token persisted and exchange not repeated. Unit `spotify::tests::authorization_expires_after_ten_minutes_and_is_consumed`: valid at 599 seconds, expired at 600, single use. | Socket compiled only; TTL/single-use unit passed |
| D4 | `d4_exchange_errors_are_safe_and_do_not_write_tokens`: invalid_grant, malicious token-bearing description and 503 temporarily_unavailable. `d4_network_failure_does_not_write_tokens`: stopped fake host. Pure `error_pages_escape_markup_and_never_repeat_upstream_tokens` checks sanitization. | Socket compiled only; pure passed |
| D5 | `d5_cache_write_failure_names_path_and_stays_signed_out`: exchange succeeds, cache write fails, path named, signed_in false. | Compiled only |
| D6 | `d6_revoked_refresh_requires_login_and_has_no_retry_loop`: reauthorize true, non-retryable failure, subsequent sync denied, token request count bounded. | Compiled only |
| D7 | `d7_d8_rate_limit_keeps_progress_and_sync_is_single_flight`: concurrent POST returns 202 with the running done/total and one read sequence. | Compiled only |
| D8 | `d7_d8_rate_limit_keeps_progress_and_sync_is_single_flight`: retryable error/retry_at while waiting, preserved progress and completion. `d8_exhausted_rate_limit_retains_retry_time_and_prior_cache`: bounded retries, deadline retained, no incomplete cache published. | Compiled only |
| D9 | `d9_d12_paged_library_lists_metadata_without_secrets`: 2,051 liked tracks, prompt 202, completion/progress, paginated lists, artwork and ordered album tracks. | Compiled only |
| D10 | `d1_d10_unconfigured_and_empty_cache_are_explicit`: empty lists and synced_at null. Pure `empty_cache_contract_and_configuration_hint` checks empty items. | Socket compiled only; pure passed |
| D11 | `d11_policy_names_the_new_operation_ids`: allowed status, denied sync/list ids named. Pure `unknown_auth_is_loopback_denial_and_policy_uses_operation_id` and OpenAPI test validate policy naming and all seven ids. | Socket compiled only; pure passed |
| D12 | `d9_d12_paged_library_lists_metadata_without_secrets`, exchange failures and helper checks across auth/sync cases scan response bodies, redirect locations, captured TRACE logs and `/actions` for fake access/refresh strings. CLI `sign_in_and_sync_make_the_dj_work_without_a_restart` scans daemon output and response/action bodies. Pure sanitization test passed. | Socket and CLI E2E compiled only; pure passed |

Additional compiled coverage: `resync_removes_unliked_tracks_from_browse_membership`.
Pure `spotify_browse_cache_survives_reopen_and_replaces_membership` passed against
MemStore and the real fsqlite store. Existing v1/v2 upgrade tests passed with
migration 7. The DJ source still calls `pool_from_store(store)` on every start;
sync receives the exact same store, not a separate cache instance.

## Operation ids for the NAS policy

| Method and path | Exact operation id |
|---|---|
| `GET /auth/spotify/login` | `spotify_login` |
| `GET /auth/spotify/callback` | `spotify_callback` |
| `GET /spotify/status` | `spotify_status` |
| `POST /spotify/sync` | `spotify_sync` |
| `GET /spotify/albums` | `list_spotify_albums` |
| `GET /spotify/albums/{id}/tracks` | `list_spotify_album_tracks` |
| `GET /spotify/tracks` | `list_spotify_tracks` |

Merge the five status/sync/list ids into the existing `[clients.unknown] allow`
list for LAN access. Preserve existing operation ids, including the app's play
and DJ operations. Auth remains loopback-only regardless of the allow list.
`docs/DEPLOY.md` includes the dashboard client id, registered redirect, SSH
tunnel, browser login, POST sync and status polling procedure.

## Remaining runtime verification

The orchestrator should execute:

```sh
cargo test -p fsonos-api --test spotify_daemon -- --nocapture
cargo test -p fsonos-cli --test e2e_spotify -- --nocapture
```

Those commands validate the fake-host HTTP exchange, background timing,
single flight, Retry-After waits, revoked refresh, token cache persistence and
write failures, captured logs/action log secrecy, and DJ playback without a
restart. Compilation and pure tests do not establish those runtime outcomes.

Real Spotify consent, dashboard eligibility, real token exchange/refresh,
the owner's library, SSH forwarding, NAS policy, Tailscale routing and physical
Sonos playback were not exercised. Existing workspace socket integration
tests also need the orchestrator's verifying pass. No deployment was performed.
