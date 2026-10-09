# Spotify app sign-in and artwork daemon report

Implemented the daemon contracts from `spotify-signin-plan.md` and `art-plan.md`
on `ios-app`. The permitted checks pass. Socket tests compiled but were not
executed, as requested. Their runtime assertions remain unverified.

This agent staged nothing, committed nothing and deleted no files. This report
is the only file this agent wrote under `ios/`. Networking uses asupersync,
routes register through the builder, and crate roots retain `forbid(unsafe_code)`.
Existing loopback login/callback restrictions and sync response statuses remain.

## Implementation

- `POST /auth/spotify/exchange` accepts the app's code, PKCE verifier and exact
  configured redirect. It uses the existing Session, token endpoint and private
  token cache. Policy-approved LAN and tailnet callers can use it. Pending Mac
  consent, an exchange in flight or a running sync makes this route answer 409.
- `/spotify/status` includes `app_redirect_uri`, and includes `client_id` only
  when configured. `FSONOS_SPOTIFY_APP_REDIRECT_URI` and
  `--spotify-app-redirect-uri` default to `frankensonos://spotify-callback`.
- A successful exchange replaces the token and clears both the DJ library and
  browse snapshot in the shared store before answering success. fsqlite clears
  both tables in one transaction; MemStore implements the same behavior. Play
  history remains. The database clear passed against memory and file stores,
  including reopen. No migration or dependency change was needed.
- Token persistence and database clearing are separate operations. If database
  clearing fails after token persistence, the route answers an error and marks
  the session signed out; the new token may already be on disk. Those operations
  are not a transaction spanning both stores.
- Exchange input parsing emits fixed error text instead of serde messages that
  could repeat submitted secrets. Upstream descriptions are discarded. Safe
  errors retain recognized OAuth error names or identify the token-cache path.
  ExchangeRequest deliberately has no Debug implementation. The shared test
  helper scans response bodies, redirect locations, captured TRACE output and
  a successfully read action log for the code, verifier and fake tokens.
- Track artwork from SOAP metadata and live state, and favorite artwork, use
  the resolved group coordinator. HTTPS artwork passes through. A valid
  `/getaa` path or HTTP URL on that exact player's IP and port 1400 becomes a
  percent-encoded relative `/art` URL. Absent artwork stays absent.
- `GET /art` looks up the player and fixes the fetch address to its IP:1400.
  Validation refuses traversal, recursively encoded traversal, authorities,
  schemes and unsafe paths before fetching. The asupersync client disables
  redirects, retries and proxies; it limits requests to five seconds and bodies
  to 4 MiB. Successful image responses preserve content-type and add
  `Cache-Control: public, max-age=86400`. Upstream 404 has no cache header.
- OpenAPI documents the exchange body, success DTO and `get_art` binary image
  response. Existing error documentation includes the added error codes and
  the route-specific status overrides.

## Files changed by this agent

| File | Change |
|---|---|
| `crates/fsonos-api/src/art.rs` | New validation, normalization and asynchronous speaker fetch |
| `crates/fsonos-api/src/failure.rs` | New codes and helper for route-specific HTTP statuses |
| `crates/fsonos-api/src/http.rs` | Builder entries, async handler wrapper and OpenAPI schemas |
| `crates/fsonos-api/src/lib.rs` | Private artwork module |
| `crates/fsonos-api/src/live.rs` | Carry live metadata artwork into TrackDto |
| `crates/fsonos-api/src/reads.rs` | Optional serialized `TrackDto.art_url` |
| `crates/fsonos-api/src/spotify.rs` | Exchange, app redirect, status fields, safe failures and pure tests |
| `crates/fsonos-api/src/surface.rs` | Normalize track and favorite artwork using the resolved player |
| `crates/fsonos-api/tests/art.rs` | Pure route/normalization tests and local speaker HTTP stub |
| `crates/fsonos-api/tests/spotify_exchange.rs` | E1 to E10 integration tests |
| `crates/fsonos-api/tests/support/spotify.rs` | Shared original daemon harness, body requests and secrecy assertions |
| `crates/fsonos-api/tests/spotify_daemon.rs` | Reuse extracted harness and update unconfigured status expectation |
| `crates/fsonos-cli/src/config.rs` | App redirect environment setting and CLI flag |
| `crates/fsonos-cli/src/daemon.rs` | Pass app redirect to Spotify service |
| `crates/fsonos-cli/src/doctor.rs` | Update ServeArgs construction |
| `crates/fsonos-cli/src/doctor/tailscale.rs` | Update ServeArgs construction |
| `crates/fsonos-cli/tests/e2e/mod.rs` | Scrub app redirect environment override in E2E harness |
| `crates/fsonos-core/src/store.rs` | Store API and MemStore library/browse clearing |
| `crates/fsonos-core/src/store/sqlite.rs` | Transactional clear of both Spotify tables |
| `crates/fsonos-core/tests/store_conformance.rs` | Account-switch clear and reopen test |
| `crates/fsonos-spotify/src/client.rs` | Exchange form variant with supplied redirect and pure test |
| `crates/fsonos-spotify/src/fake_spotify.rs` | Verify exact redirect and supplied PKCE challenge; delay for concurrency test |
| `crates/fsonos-spotify/src/session.rs` | App exchange using existing cache and token request path |
| `docs/DEPLOY.md` | Setting, phone flow, artwork and policy ids |
| `docs/ERRORS.md` | Added codes and route-specific failure statuses |
| `ios/docs/spotify-signin-daemon-report.md` | This report |

The pre-existing untracked `docs/spotify-daemon-worklog.md` was not edited.
Other agents' iOS changes were preserved.

## Tests first and actual red output

The new integration tests were written before production implementation.
Compile-red was captured with:

```sh
CARGO_TARGET_DIR=/private/tmp/frankensonos-codex-target \
  cargo test -p fsonos-api --test spotify_exchange --test art --no-run
```

Selected output from `/private/tmp/spotify-signin-art-red.log` (exit 101):

```text
error[E0061]: this function takes 4 arguments but 5 arguments were supplied
   --> crates/fsonos-api/tests/support/spotify.rs:86:23
error[E0609]: no field `art_url` on type `fsonos_api::TrackDto`
   --> crates/fsonos-api/tests/art.rs:152:76
error: could not compile `fsonos-api` (test "spotify_exchange") due to 1 previous error
error: could not compile `fsonos-api` (test "art") due to 1 previous error
```

The store and exchange-form pure tests were also added before their methods.
Selected compile-red from `/private/tmp/spotify-signin-pure-red.log` (exit 101):

```text
error[E0599]: no method named `clear_spotify_library` found for mutable reference `&mut dyn fsonos_core::store::Store` in the current scope
error: could not compile `fsonos-core` (test "store_conformance") due to 1 previous error
error[E0599]: no method named `code_exchange_body_with_redirect` found for struct `client::SpotifyConfig` in the current scope
error: could not compile `fsonos-spotify` (lib test) due to 1 previous error
```

A supplemental pure OpenAPI assertion was written before fixing its schema.
Runtime-red from `/private/tmp/spotify-signin-art-openapi-red.log`:

```text
assertion `left == right` failed
  left: Null
 right: "binary"
test pure_a7_policy_and_openapi_operation ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.03s
```

This establishes compile-red for the new socket suites and behavioral red for
the pure schema case. It does not establish behavioral socket-red. Additional
input, secrecy and concurrency assertions were added during implementation.

## Verification and gate tails

All Cargo build/test commands used
`CARGO_TARGET_DIR=/private/tmp/frankensonos-codex-target`. `rch` and `br` were
unavailable, so gates ran locally; no beads were claimed or closed. Shared
memory recall was restricted, and the final CLI handoff write failed with
`journal_unavailable` at the adapter journal outside the writable roots. No
memory ID was returned. The local worklog is
`/private/tmp/frankensonos-signin-art-worklog.md`.

| Check | Result |
|---|---|
| `cargo fmt --check` | Exit 0, no output |
| `cargo check --workspace --all-targets` | Exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo test --workspace --no-run` | Exit 0; all workspace test targets compiled |
| Selected pure tests and non-socket CLI checks, below | Exit 0; 498 passed, 0 failed, 27 filtered across 26 binaries |
| `cargo test --workspace --doc` | Exit 0; one `no_run` example compiled |
| `git diff --check` | Exit 0 |
| `cargo test --workspace` | Not executed: contains socket tests |

Actual check tail, `/private/tmp/spotify-signin-art-check.log`:

```text
    Checking fsonos-api v0.1.0 (/Users/samuelreed/git/forks/frankensonos/crates/fsonos-api)
    Checking fsonos-mcp v0.1.0 (/Users/samuelreed/git/forks/frankensonos/crates/fsonos-mcp)
    Checking fsonos-cli v0.1.0 (/Users/samuelreed/git/forks/frankensonos/crates/fsonos-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.61s
```

Actual clippy tail, `/private/tmp/spotify-signin-art-clippy.log`:

```text
    Checking fsonos-api v0.1.0 (/Users/samuelreed/git/forks/frankensonos/crates/fsonos-api)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.47s
```

Selected actual compile-only lines, `/private/tmp/spotify-signin-art-no-run.log`:

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.22s
  Executable tests/art.rs (/private/tmp/frankensonos-codex-target/debug/build/fsonos-api/5f768e502301ce4a/out/art-5f768e502301ce4a)
  Executable tests/spotify_daemon.rs (/private/tmp/frankensonos-codex-target/debug/build/fsonos-api/a747489490860a3a/out/spotify_daemon-a747489490860a3a)
  Executable tests/spotify_exchange.rs (/private/tmp/frankensonos-codex-target/debug/build/fsonos-api/0b20f4364e101eb4/out/spotify_exchange-0b20f4364e101eb4)
  Executable tests/e2e_spotify.rs (/private/tmp/frankensonos-codex-target/debug/build/fsonos-cli/5e9f21ad70182b6a/out/e2e_spotify-5e9f21ad70182b6a)
```

Executed pure/non-socket command:

```sh
CARGO_TARGET_DIR=/private/tmp/frankensonos-codex-target \
  cargo test --workspace --lib --bins \
  --test control --test events --test favorites --test fsqlite_roundtrip \
  --test playback --test reconcile --test rooms_resolution --test search \
  --test store_conformance --test survey --test topology_golden \
  --test spotify_contract --test art --test golden --test status_golden \
  --test exit_codes --test mcp_stdio -- \
  --skip net::tests:: --skip feed::tests:: --skip session::tests:: \
  --skip sync_library_end_to_end_against_fake_spotify \
  --skip albums_are_read_once_through_429_and_paging_then_cached \
  --skip a_rate_limit_stops_the_run_and_unplayable_albums_wait \
  --skip a_missing_daemon_is_a_warning_with_a_remedy \
  --skip routes_and_banner_cover_every_player_on_loopback \
  --skip single_household_scenarios --skip socket_
```

Actual artwork result from `/private/tmp/spotify-signin-art-pure.log`:

```text
test pure_track_dto_carries_optional_art ... ok
test pure_a1_a2_a8_unknown_player_and_unsafe_paths ... ok
test pure_a7_policy_and_openapi_operation ... ok
test pure_a6_track_and_favorite_art_normalization ... ok
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.10s
```

The 27 filtered cases are eight protocol network unit tests, fifteen Spotify
socket unit tests, three CLI socket unit tests and the new artwork socket test.
Socket integration targets, including `spotify_exchange`, were omitted from
execution. No socket test was attempted by this agent.

The first selected pure run failed the existing error-document order check
after adding the new codes; that run reported 79 API tests passed and one
failed. The table was corrected and the final run passed all 80 API unit tests.
The failed run and supplemental schema-red above are not counted as passes.
Initial compiler/lint findings were fixed before the final gates recorded here.

Doctest evidence from `/private/tmp/spotify-signin-art-doc.log`:

```text
   Doc-tests fsonos_sim
running 1 test
test crates/fsonos-sim/src/lib.rs - (line 13) - compile ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## E1 to E10 coverage

Exchange integration names below are in
`crates/fsonos-api/tests/spotify_exchange.rs`. Compiled means the assertions
built, not that they passed at runtime. All eight new exchange tests are socket
tests and remain unexecuted.

| Row | Contract and test coverage | Executed here |
|---|---|---|
| E1 | `e1_status_and_unconfigured_exchange`: 503 `SPOTIFY_NOT_CONFIGURED`, no client_id, default app redirect, signed out, no upstream request. Unit `status_reports_configured_app_redirect_and_client_id` checks configured/unconfigured status and custom redirect. Existing pure `spotify_contract::empty_cache_contract_and_configuration_hint` checks the configuration failure and hint. | Exchange compiled only; status unit and pure contract passed |
| E2 | `e2_e3_invalid_inputs_never_reach_spotify_or_echo_secrets`: loopback, other scheme callback and trailing slash refused with 400 `SPOTIFY_REDIRECT_NOT_ALLOWED`, no upstream request. | Compiled only |
| E3 | Same integration test: short/long/invalid/non-ASCII verifier, empty/oversized code, wrong JSON types and malformed JSON yield 400 without sending secrets. Unit `app_verifier_and_code_boundaries_are_validated_without_echoing_input` also checks 43/128 verifier and 1/512 code boundaries. | Integration compiled only; boundary unit passed |
| E4 | `e4_pkce_and_redirect_are_verified_by_fake_and_errors_are_safe`: expired code, wrong verifier, exact redirect mismatch at fake and malicious upstream description produce safe 400; token cache empty and signed out. Fake checks S256 against the test-supplied RFC challenge and exact redirect. Existing unit `error_pages_escape_markup_and_never_repeat_upstream_tokens` checks sanitization. | Integration compiled only; sanitization unit passed |
| E5 | `e5_upstream_and_network_failures_are_retryable`: fake 503 and stopped fake endpoint produce 502 with retryable true, no persisted token and secrecy scan. | Compiled only |
| E6 | `e6_pending_login_exchange_and_sync_are_busy`: pending Mac consent, in-flight app exchange and running sync each deny another app exchange with 409. Existing Mac login and sync busy statuses remain 503. Existing TTL/single-use authorization unit also passed. | Integration compiled only; TTL unit passed |
| E7 | `e7_cache_failure_names_path_and_stays_signed_out`: an unwritable token-cache temporary path yields 500, names cache path, status signed out and no token. | Compiled only |
| E8 | `e8_e9_allowed_remote_exchange_replaces_token_and_clears_both_caches`: seed previous library, replace saved token, verify empty album/liked listings and library search plus null synced_at immediately after success. Core `account_switch_clears_library_and_browse_preserves_history_and_survives_reopen` checks both stores and reopen. | HTTP sequence compiled only; core conformance passed |
| E9 | E8 test and helpers throughout exchange suite scan bodies, locations, captured TRACE logs and `/actions` for code, verifier, old and new tokens. Malicious errors, malformed/type-invalid bodies and failure responses are scanned too. Pure validation and sanitization tests check safe error text. | Capture/scans compiled only; pure error checks passed |
| E10 | `e10_policy_denies_exchange_without_upstream_request`: 403 `POLICY_DENIED` names `spotify_exchange`, upstream log empty. E8 also succeeds with an explicitly allowed Unknown caller. | Compiled only |

Pure `app_exchange_body_uses_supplied_redirect_and_encodes_secrets` passed. It
checks the exact custom redirect, grant fields and escaped code/verifier in the
form body. Existing RFC 7636 vector and authorization URL pure tests passed.

## A1 to A8 coverage

All artwork tests below are in `crates/fsonos-api/tests/art.rs`.

| Row | Contract and test coverage | Executed here |
|---|---|---|
| A1 | `pure_a1_a2_a8_unknown_player_and_unsafe_paths`: unknown player gives 404 `UNKNOWN_PLAYER`. Refusal uses a surface without network transport. | Passed |
| A2 | Same pure test: invalid prefix, wrong scheme, authorities, slashes, traversal, invalid query and oversized u yield 400 `INVALID_ARGUMENT`. Socket test additionally confirms refused paths never reach the stub. | Pure passed; upstream count assertions compiled only |
| A3 | `socket_a3_a4_a5_images_limits_timeout_and_no_redirects`: six-second stub delay returns 502 retryable before six seconds; stopped stub also returns 502 retryable. Redirect response does not trigger another request. | Compiled only |
| A4 | Same socket test: text/html and 4 MiB + 1 byte return 502 `BAD_ART`; exactly 4 MiB succeeds. Positive image bytes, content-type and cache header are checked. | Compiled only |
| A5 | Same socket test: upstream 404 returns 404 without cache-control. | Compiled only |
| A6 | `pure_a6_track_and_favorite_art_normalization`: missing track artwork is absent, HTTPS passes through, speaker path and exact IP:1400 URL become encoded daemon URL, wrong IP/port omitted. `pure_track_dto_carries_optional_art` checks optional DTO field. | Both passed |
| A7 | `pure_a7_policy_and_openapi_operation`: policy denial names `get_art`; OpenAPI has the exact id and binary image schema. | Passed |
| A8 | A1/A2 pure test includes literal, encoded and doubly encoded traversal, `//host`, `@`, backslash, fragment and encoded controls. Socket test asserts refused paths do not reach the speaker. Address comes only from discovered player, never from u. | Pure passed; socket assertions compiled only |

## Exact operation ids for the NAS policy

Add these two ids to the existing app caller's allow list, preserving all
already allowed status, sync, browse and playback operations:

| Method and route | Exact operation id |
|---|---|
| `POST /auth/spotify/exchange` | `spotify_exchange` |
| `GET /art?player=&u=` | `get_art` |

`GET /spotify/status` retains `spotify_status`. Existing Mac auth ids remain
`spotify_login` and `spotify_callback`, and those routes retain their loopback
restriction regardless of policy. This agent did not edit or deploy NAS policy.

## Remaining runtime verification

The orchestrator should run:

```sh
cargo test -p fsonos-api --test spotify_exchange -- --nocapture
cargo test -p fsonos-api --test art -- --nocapture
cargo test -p fsonos-api --test spotify_daemon -- --nocapture
cargo test -p fsonos-cli --test e2e_spotify -- --nocapture
```

The artwork stub binds `127.0.0.1:1400`, matching the fixed speaker-port
contract. That port must be free for this test. Its pure tests run without
opening sockets; the `socket_` test is the one excluded locally.

These runs must establish HTTP exchange, PKCE/redirect enforcement at the fake,
concurrency timing, cache-write failure responses, captured-log/action secrecy,
account switching and image transport behavior. Existing workspace socket
tests also need the orchestrator's verifying pass.

Real Spotify consent/token exchange, the phone sheet, NAS/Tailscale policy and
routing, real speaker image fetch and event-driven artwork refetch were not
exercised. No deployment or live-device operation was performed.
