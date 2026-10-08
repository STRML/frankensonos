# Live store report

Implemented on `ios-app`. The headless checks compile, the iOS Simulator build
succeeds, and the existing mock check and screenshot renderer pass. No E2E row
is claimed to pass. The orchestrator will run the real daemon scenarios outside
this sandbox and return failures for repair.

## Files added and changed

Paths below are relative to `ios/`.

| Files | Change |
| --- | --- |
| `FrankenSonos/Net/DaemonClient.swift` | URLSession JSON HTTP, encoded room paths and favorite queries, structured errors, SSE bytes/lines, resume header, retry delays and monotonic 40-second silence watchdog. |
| `FrankenSonos/Net/DTOs.swift` | Decoders matching the observed zones, rooms, state, track, favorites, deltas and error contracts. Optional fields stay optional; unknown fields are ignored. |
| `FrankenSonos/Net/SSEParser.swift` | Comment handling, id/event/data fields, multiline data and blank-line dispatch. |
| `FrankenSonos/Model/LiveZoneStore.swift` | Replaces the stub with published live state, bootstrap/reconnect lifecycle, background cancellation and local position ticking. |
| `FrankenSonos/Model/LiveZoneStore+Snapshot.swift` | Rooms, topology, per-room states and favorites; snapshot revision checks; coordinator identity and position reconciliation. |
| `FrankenSonos/Model/LiveZoneStore+Events.swift` | Transport/volume deltas, health and queue count; coalesced state/topology reads; reset and unknown-room refresh. |
| `FrankenSonos/Model/LiveZoneStore+Volume.swift` | Optimistic volume, 100 ms debounce, serial writes per room, echo suppression during editing, release reconciliation and failure revert. |
| `FrankenSonos/Model/LiveZoneStore+Commands.swift` | Pause/resume/next/previous, favorites, optimistic grouping, per-household party mode and ungroup all. |
| `FrankenSonos/Model/StableIdentity.swift` | Deterministic UUID per household/coordinator; bounded cover seed. |
| `FrankenSonos/Model/DaemonSettings.swift`, `ZoneConnectionStatus.swift` | Persistent daemon URL with default `http://127.0.0.1:8099`, validation and status states. |
| `FrankenSonos/Model/Track.swift` | Real live duration and optional favorite/art fields. Presentation fields are excluded from headless compilation. Mock catalog, palettes and durations remain intact. |
| `FrankenSonos/Model/MockZoneStore.swift` | Optional forwarding adapter for existing view inputs; mock initialization remains the renderer default. |
| `FrankenSonos/FrankenSonosApp.swift` | Live default, `-mock` selection and scene lifecycle. |
| `FrankenSonos/Views/ConnectionBanner.swift` | Offline URL/Retry, reconnecting and refreshing banner. |
| `FrankenSonos/Views/SettingsView.swift`, `RemoteChrome.swift` | URL field, host/status, connection banner, daemon error details/hints and suggestion buttons; slider editing callbacks. |
| `FrankenSonos/Views/RoomsView.swift`, `GroupRoomsSheet.swift`, `MiniPlayerBar.swift`, `NowPlayingView.swift` | Offline controls, per-room slider lifecycle, queue count, stream display and live position presentation. |
| `FrankenSonos/Views/BrowseView.swift`, `MySonosView.swift`, `SearchView.swift`, `AlbumArtworkView.swift` | Live favorites and optional artwork. Mock content and layout remain conditional defaults. |
| `Package.swift` | `FSONOS_LIVE_CHECKS=1` selects a macOS Model/Net library and XCTest target without SwiftUI. Default selects SketchShots. |
| `Tests/LiveChecks.swift` | The pre-implementation scenarios now compile. Row 15 is split into decoder, SSE-fixture and unknown-event helpers. Favorite verification waits for the daemon before issuing the next command. |
| `tools/e2e-live` | Preliminary harness from the RED phase. Orchestrator confirmed it starts isolated sim/serve on free ports. No Rust build, existing services untouched. |
| `project.yml`, `Generated/Info.plist` | XcodeGen emits local-network ATS permission and an insecure HTTP exception for `ts.net` including subdomains. No arbitrary-load permission. |
| `docs/live-store-report.md` | This report. |

The `ZoneStore` protocol shape, view initializer inputs and SketchRenderer remain
unchanged. Existing views receive the same concrete environment object, which
forwards to LiveZoneStore when initialized for live use. Duplicate room names
receive `@household` labels; commands always use qualified room targets.

## RED evidence

The initial sandbox harness attempt failed before opening its loopback socket:

```text
Operation not permitted at -e line 1.
FAIL row 1: infrastructure or compilation prevented this scenario
```

That was infrastructure failure. The orchestrator subsequently ran the harness
outside this sandbox and confirmed simulator startup, daemon readiness, and real
compiler RED against the absent API. This is orchestrator-provided evidence;
the external run's complete output was not supplied here.

The locally captured pre-implementation compiler diagnostics included:

```text
Tests/LiveChecks.swift:41:22: error: cannot find 'DaemonClient' in scope
Tests/LiveChecks.swift:42:44: error: argument passed to call that takes no arguments
Tests/LiveChecks.swift:158:31: error: cannot find 'DaemonEvent' in scope
Tests/LiveChecks.swift:143:9: error: failed to produce diagnostic for expression
error: Build failed
error: fatalError
```

Original log: `/private/tmp/frankensonos-live-tests-red.log`.
The large row-15 expression is now small helper calls, and compilation succeeds.

## Local verification

All following final commands exited 0. They do not require a daemon socket.

### Headless checks compile

```sh
cd ios
FSONOS_LIVE_CHECKS=1 \
SWIFTPM_MODULECACHE_OVERRIDE=/private/tmp/frankensonos-module-cache \
CLANG_MODULE_CACHE_PATH=/private/tmp/frankensonos-module-cache \
swift build --disable-sandbox --build-tests
```

Actual final tail:

```text
[14 / 19] LiveChecks-product
[16 / 21] LiveChecks-product
[18 / 21] LiveChecks-product
[20 / 21] LiveChecks-product
Build complete! (1.74 sec)
```

No XCTest scenario was executed. SwiftPM emitted warnings about unavailable
user caches and a read-only manifest cache; they did not prevent compilation.

### Project generation and Simulator build

```sh
cd ios
xcodegen generate
xcodebuild build -project FrankenSonos.xcodeproj -scheme FrankenSonos \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath /private/tmp/frankensonos-derived \
  OTHER_SWIFT_FLAGS='-Xfrontend -disable-sandbox' CODE_SIGNING_ALLOWED=NO
```

Generation output:

```text
⚙️  Generating plists...
⚙️  Generating project...
⚙️  Writing project...
Created project at /Users/samuelreed/git/forks/frankensonos/ios/FrankenSonos.xcodeproj
```

Actual final build tail:

```text
Ignoring --strip-bitcode because --sign was not passed

PruneExplicitPrecompiledModules /tmp/frankensonos-derived/Build/Intermediates.noindex/SwiftExplicitPrecompiledModules

PruneExplicitPrecompiledModules /tmp/frankensonos-derived/Build/Intermediates.noindex/ExplicitPrecompiledModules

PruneExplicitPrecompiledModules /tmp/frankensonos-derived/SDKExplicitPrecompiledModules

** BUILD SUCCEEDED **
```

The first build found a missing return in the adapter's `model(for:)`; fixed
with `return switch` before the successful builds. CoreSimulator service
warnings occurred during the build. No Simulator was booted. Both generated
and built Info.plists were inspected for the scoped ATS dictionary.

### Existing mock-state check

```sh
cd ios
swiftc -swift-version 5 -Xfrontend -disable-sandbox \
  -module-cache-path /private/tmp/frankensonos-module-cache \
  FrankenSonos/Model/*.swift FrankenSonos/Net/*.swift \
  check-mock-state.swift -o /private/tmp/frankensonos-live-mock-check
/private/tmp/frankensonos-live-mock-check
```

Actual output:

```text
PASS: room inventory, unique IDs, selected target, unrelated groups, per-room volume, party mode, pause all, ungroup, track selection, sleep timer target/expiry, 40 regroup operations
```

The existing assertions are unchanged. The compile includes the adapter's live
references; MockZoneStore() does not start networking.

### Mock screenshots

```sh
cd ios
SWIFTPM_MODULECACHE_OVERRIDE=/private/tmp/frankensonos-module-cache \
CLANG_MODULE_CACHE_PATH=/private/tmp/frankensonos-module-cache \
swift run --disable-sandbox SketchShots /private/tmp/frankensonos-live-final-shots
```

Actual final renderer tail:

```text
/private/tmp/frankensonos-live-final-shots/settings.png (780x1688)
/private/tmp/frankensonos-live-final-shots/search.png (780x1688)
/private/tmp/frankensonos-live-final-shots/search-selected-room.png (780x1688)
/private/tmp/frankensonos-live-final-shots/overview.png
Rendered 12 screens at 780x1688 and overview.png at 3120x5064
```

Compared every screen against the prior captures copied before editing. All
pixels outside the existing animated EQ regions match exactly. Settings and
both group-sheet captures are pixel-identical. The largest final difference is
204 pixels in Rooms dark's animated EQ bars. The contact sheet was visually
inspected; layout, typography and mock artwork retain the baseline look.

Comparison tail:

```text
PASS settings.png: 780x1688, 0 changed pixels in animated EQ only
PASS: all 12 screens match baseline outside animated EQ; overview 3120x5064
```

`bash -n tools/e2e-live`, `git diff --check`, and the added-source em-dash scan
also passed. No Rust or Cargo files were modified. No commit or push.

## Matrix status

Every row has an implementation path. Twelve row scenarios compile in the
headless target, but **none has run after implementation**. The six remaining
rows have code only. No E2E pass or mutation result is inferred from a build.

| Row | Implementation | Coverage and remaining proof |
| --- | --- | --- |
| 1 | Empty/offline launch, banner, Retry and 1/2/4/8/15-second backoff. | Compiled scenario, not run: unreachable launch, no sample rooms, startup recovery. Backoff timing not asserted. |
| 2 | SSE reconnect with Last-Event-ID and full state reconciliation. A confirmed daemon outage clears the volatile cursor. | Compiled scenario, not run: stop/restart serve and subsequent external volume event. Header replay itself is not inspected. |
| 3 | Monotonic 40-second silence watchdog closes the stream and retries. | Code only. No silent-stream test. |
| 4 | Reset clears cursor, shows Refreshing and schedules full topology/per-room refetch. | Code only. No ring-overflow scenario. |
| 5 | Unknown room schedules full topology and room-state refresh. | Code only. No topology-race scenario. |
| 6 | Revert volume/playback/group optimism; daemon detail/hint and suggestion UI. | Compiled scenario, not run: bad room volume revert and detail. Suggestion/hint interaction is not tested. |
| 7 | Command network/5xx failure reverts, refetches and reports Didn't go through. | Compiled scenario, not run: unreachable command. Timeout and HTTP 5xx are not separate scenarios. |
| 8 | Optimistic per-room volume, 100 ms debounce, serial latest writes, editing echo suppression and release reconciliation. | Compiled scenario, not run: immediate value and final daemon volume. In-flight/release races, echo-during-drag and request counts need proof. |
| 9 | Optimistic group/ungroup, idempotent topology refetch after commands; party mode stays within one household. | Compiled scenario, not run: group/ungroup, daemon comparison and retained coordinator ID. Partial failures and coordinator removal need proof. |
| 10 | Favorite command plus URI-triggered zone-state refetch coalesced for 150 ms. | Compiled scenario, not run: favorite and external track metadata update. Request-count/coalescing timing is not asserted. |
| 11 | Optional metadata and zero duration; live streams hide scrubber and show Live. | Compiled scenario, not run: captured stopped/stream DTOs and missing metadata. UI only compiled. |
| 12 | Local position anchor plus elapsed time, capped for known duration and resynced by state reads. | Compiled scenario, not run: duration mapping, basic nondecreasing tick, pause/resume. Positive-duration progression/drift proof is still needed. |
| 13 | UTF-8 path segment encoding, including space and plus; favorite queries encode plus too. | Compiled scenario, not run: encoded special URL and actual space-containing room route. Accented/plus room exists only in URL assertion. |
| 14 | All households read; collisions use Name@household, UUIDs include household; qualified command targets. | Code only. S2 harness does not prove two-household behavior. Ambiguous raw health events conservatively dim every matching room. |
| 15 | Unknown JSON fields/event kinds ignored; SSE framing and optional fields decoded. | Compiled scenario, not run: captured JSON/SSE, future JSON field and unknown event. Row-15 compiler crash fixed by splitting helpers. |
| 16 | Offline health publishes dim/disabled room controls. Known speaker failures during bootstrap retain the inventory and mark affected rooms offline. | Code only. No offline-player runtime scenario. |
| 17 | Background cancels stream/work; foreground fully bootstraps before reconnect. | Compiled scenario, not run: store cancellation and foreground volume refresh. Native scene transitions untested. |
| 18 | Saved/editable URL, host/status row, default URL and scoped generated ATS. | Code only. Plist inspected; persistence interaction and iOS HTTP behavior untested. |

## E2E and mutations

Per the user's revised direction, `tools/e2e-live` and daemon curl requests were
not attempted during implementation. The orchestrator owns the external run.

| Required verification | Status |
| --- | --- |
| Real sim/serve matrix after implementation | Awaiting orchestrator. |
| Break optimistic revert and observe row 6/7 failure | Not run. |
| Break SSE reconnect and observe row 2 failure | Not run. |
| Restore mutated files and show clean restoration diff | No mutations applied yet. |

## Unverified and remaining limits

Real speakers, iPhone runtime, tailnet HTTP/ATS, local-network permission,
foreground/background timing, gestures, VoiceOver and favorite artwork loading
remain unverified. Mock renders are macOS-hosted captures, not native iPhone
runtime evidence. Only the release daemon/simulator E2E run can establish that
controls reach the daemon and speakers.

Live Search filters fetched favorites locally; it does not call the uncaptured
library-search endpoint. Favorites from another household are refused by the
store with a message asking for this room's household. Queue count depends on receiving a queue-length event;
otherwise the UI says it is unavailable. No queue list, service Browse tree or
seek operation was invented. The live scrubber is read-only. The sleep timer is
local to the app and is not guaranteed to fire while iOS suspends it.

`br` is unavailable. Shared memory retention failed with `journal_unavailable`;
this report and `/private/tmp/frankensonos-live-worklog.md` preserve recovery.

## Orchestrator E2E run (outside the Codex sandbox)

Codex could not open loopback sockets, so `ios/tools/e2e-live` was run from an unsandboxed shell against a real
`fsonos sim` and `fsonos serve` (release build). The first run had 10 failures. Root causes, all fixed:

| Failure | Root cause | Fix |
|---|---|---|
| Rows 1, 7 and everything after | Harness restart raced the old daemon: the control loop runs in a subshell, so `wait` did not see the daemon and the new one hit `Address already in use` | `tools/e2e-live` polls until the old daemon is gone and retries the bind |
| Rows 2, 10 | SSE reader looped over `bytes.lines`, which drops empty lines; a blank line is what ends an SSE frame, so no event was ever dispatched | `Net/DaemonClient.swift` splits the byte stream on newlines itself |
| Row 9 | A group or ungroup made while another was in flight was silently dropped by `guard !grouping` | the newest request is queued and runs when the in-flight one finishes (`queuedGrouping`) |
| Row 12 | Test read the daemon before the pause POST landed | test polls with `waitForTransport` |
| Row 17 | After a resume with nothing missed, the daemon sends no response headers until its next heartbeat (up to 15 s), so the status stayed "refreshing" with fresh data | a good refetch sets `.live`; a failed stream drops back to reconnecting |

Result: `Live checks: 0 failed`, 12 of 12 rows PASS (1, 2, 6, 7, 8, 9, 10, 11, 12, 13, 15, 17).
Rows 3, 4, 5, 14, 16 and 18 have no E2E row (see the row table above).

Mutation checks, both run and restored (`cmp` against a saved copy):

| Mutation | Rows that went red |
|---|---|
| volume revert removed (`roomVolumes[room] = edit.original`) | 6 and 7 directly; 12, 13 and 17 followed because row 7 aborted before restarting the daemon |
| reconnect loop gives up after the first failure | 1 ("live bootstrap"); the remaining rows did not run |

Also re-run after the fixes: `xcodegen generate` and the iOS Simulator `xcodebuild` print `** BUILD SUCCEEDED **`,
the mock-state check prints its PASS line, and the renderer writes 12 screens plus `overview.png`.
Still unverified: anything on an iPhone or simulator (drag-to-group, long-press, background/foreground on iOS),
real speakers, and a daemon reached over Tailscale.

Logs: `/private/tmp/frankensonos-live-{compile-green,xcodegen,build,mock-check,render,screenshot-check}.log`.
Final images: `/private/tmp/frankensonos-live-final-shots/`.
