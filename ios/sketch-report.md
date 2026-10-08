# FrankenSonos iOS S1 polish

This report lives at `ios/sketch-report.md`.

The polish pass fixes sheet footers and room-chip overflow, adds Settings and Search, and strengthens Rooms secondary text and EQ contrast. The final renderer and Simulator build passed. All 12 screen PNGs and `overview.png` were opened and visually inspected after the final render.

![All screens and sheet scroll states](sketch-shots/overview.png)

## Changes by screen

Each screen is rendered from the shared mock SwiftUI views.

| Screen | Changes and inspection results |
| --- | --- |
| [Rooms](sketch-shots/rooms.png) | Retains eight dense rows representing nine fictional speakers. Secondary text now uses an explicit opaque palette. Paused titles and subtitles no longer share a dimming modifier; paused artwork still dims. All rows clear the mini-player. |
| [Rooms dark](sketch-shots/rooms-dark.png) | Charcoal surface, explicit secondary text and EQ colors. Inspected selected, playing, and paused rows. Contrast measurements are below. |
| [Browse](sketch-shots/browse.png) | Room strip begins at 16pt and fades over its last 32pt. Full-width chip labels scroll horizontally. All seven service rows remain visible. |
| [My Sonos](sketch-shots/my-sonos.png) | Uses the corrected room strip. Recent covers and song rows retain their alignment above the mini-player. |
| [Now Playing](sketch-shots/now-playing.png) | Uses the same corrected strip. Cover, scrubber, transport, volume, room pill, queue, and sleep controls remain fully clear. |
| [Group Rooms, default](sketch-shots/group-rooms.png) | Clipped vertical ScrollView with 60pt rows and 4pt gaps. Ungroup all is pinned in a bottom safe-area inset with its own opaque surface. Eight complete rooms fit at the default 688pt sheet height. A bottom fade and Scroll rooms caption indicate remaining content. |
| [Group Rooms, end](sketch-shots/group-rooms-scrolled-end.png) | The actual ScrollView scrolls to an end marker after Movie Room. Movie Room artwork, model, volume, and checkbox clear the pinned footer. The fade falls in the end padding. |
| [Rooms & Volume, default](sketch-shots/now-playing-rooms-sheet.png) | Native iOS sheet offers `.medium` and `.large` detents, selected medium initially and reset on dismissal. The shared list has an inset footer for Party mode and Pause all. Bottom fade, peeking Dining Room, and a scroll/expand hint explain the compact viewport. The macOS host approximates the medium detent with a 400pt frame. |
| [Rooms & Volume, end](sketch-shots/now-playing-rooms-sheet-scrolled-end.png) | The actual list scrolls to its end, showing Bathroom, Garage, and Movie Room fully above the fixed actions. |
| [Settings](sketch-shots/settings.png) | S1 grouped list with System, Services and Voice, Account, and Help. Monochrome icons, disclosure chevrons, inset separators, gray section headers, and mock summaries. Rows open a local preview alert. Corrected a trailing background gutter found during inspection. |
| [Search](sketch-shots/search.png) | Search field, recent searches with Clear, service filter chips, and local song results. Keeps the room switcher and mini-player. Query and service selection filter mock tracks. |
| [Search, changed room](sketch-shots/search-selected-room.png) | Renderer changes selection from Kitchen to Movie Room after the initial layout. The strip auto-scrolls and shows the selected Movie Room chip fully. The mini-player also follows selection. |

The strip uses `ScrollViewReader`, stable room IDs, and selection-change scrolling on the next UI update. Inspection caught an animated scroll request that did not move the macOS host; the final capture verifies the corrected request. Leading spacing sits outside the scroll content so initial scrolling preserves the 16pt alignment. Its trailing gradient is a mask on the shared view.

## Contrast measurements

The check uses the WCAG sRGB formula: linearize each channel, calculate relative luminance, then divide `(lighter + 0.05)` by `(darker + 0.05)`. The AA threshold is 4.5:1 for the requested 12pt text; the Rooms subtitles are 11pt and use that same threshold. EQ bars also meet 4.5:1.

`S1Palette` defines opaque secondary text at RGB 0.72 in dark mode and EQ at RGB 0.88. Rounded 8-bit colors are `#B8B8B8` and `#E0E0E0`. Pixel sampling of the final dark Rooms PNG established the normal `#131313` background and selected `#181818` background.

| Element | Foreground / background | Contrast | AA |
| --- | --- | --- | --- |
| Dark secondary text, normal and paused rows | `#B8B8B8` / `#131313` | 9.37:1 | Pass |
| Dark secondary text, selected row | `#B8B8B8` / `#181818` | 8.95:1 | Pass |
| Dark EQ, normal row | `#E0E0E0` / `#131313` | 14.07:1 | Pass |
| Dark EQ, selected row | `#E0E0E0` / `#181818` | 13.45:1 | Pass |
| Light secondary text, selected row | `#616161` / `#F9F9F9` | 5.88:1 | Pass |
| Light EQ, selected row | `#333333` / `#F9F9F9` | 12.00:1 | Pass |

As a raster cross-check, the brightest sampled text pixels in the selected subtitle crop `(148,335)-(500,359)` were `#B5B5B5`, giving 8.66:1 against the sampled background at `(500,320)`. The paused subtitle crop `(148,459)-(500,484)` reached `#B2B2B2`, giving 8.76:1 against `(500,430)`. Selected EQ pixels in `(588,300)-(615,332)` reached `#E0E0E0`, giving 13.45:1. Coordinates are pixels in the 2x PNG. Antialiased edge pixels blend with the background; the WCAG palette checks use the defined foreground color.

## Generate and build

Run from `ios/`:

```sh
xcodegen generate
xcodebuild build -project FrankenSonos.xcodeproj -scheme FrankenSonos \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath /private/tmp/frankensonos-derived \
  OTHER_SWIFT_FLAGS='-Xfrontend -disable-sandbox' CODE_SIGNING_ALLOWED=NO
```

Final project generation exited 0:

```text
⚙️  Generating plists...
⚙️  Generating project...
⚙️  Writing project...
Created project at /Users/samuelreed/git/forks/frankensonos/ios/FrankenSonos.xcodeproj
```

Real tail of the final Simulator build, exit 0:

```text
Ignoring --strip-bitcode because --sign was not passed

PruneExplicitPrecompiledModules /tmp/frankensonos-derived/Build/Intermediates.noindex/SwiftExplicitPrecompiledModules

PruneExplicitPrecompiledModules /tmp/frankensonos-derived/Build/Intermediates.noindex/ExplicitPrecompiledModules

PruneExplicitPrecompiledModules /tmp/frankensonos-derived/SDKExplicitPrecompiledModules

** BUILD SUCCEEDED **

```

## Render and verify

Run from `ios/`:

```sh
SWIFTPM_MODULECACHE_OVERRIDE=/private/tmp/frankensonos-module-cache \
  swift run --disable-sandbox SketchShots "$PWD/sketch-shots"
```

The final renderer exited 0 and printed:

```text
Rendered 12 screens at 780x1688 and overview.png at 3120x5064
```

Pillow opened every PNG and asserted dimensions: all 12 screens are 780x1688 (390x844 points at 2x). The overview is a four-column, three-row contact sheet at 3120x5064, preserving each screen's 2x pixels. All 13 PNGs were then opened individually for visual review. Inspection corrected the initial chip inset, footer fade touching Movie Room's slider, Settings gutter, and the selection auto-scroll.

The existing mock-state check was compiled and executed:

```sh
swiftc -swift-version 5 -Xfrontend -disable-sandbox \
  -module-cache-path /private/tmp/frankensonos-module-cache \
  FrankenSonos/Model/{Track,ZoneStore,MockZoneStore}.swift \
  check-mock-state.swift -o /private/tmp/frankensonos-mock-check
/private/tmp/frankensonos-mock-check
```

It exited 0:

```text
PASS: room inventory, unique IDs, selected target, unrelated groups, per-room volume, party mode, pause all, ungroup, track selection, sleep timer target/expiry, 40 regroup operations
```

## Unverified behavior and scope

- iOS runtime gestures remain unverified. No Simulator was booted: detent expansion/collapse, scroll-versus-sheet drag arbitration, dismissal gestures, chip taps/long presses, drag grouping, keyboard behavior, haptics, VoiceOver, Reduce Motion, and all Dynamic Type sizes were not exercised on iOS.
- Screenshots capture shared SwiftUI views in a macOS host. The native iOS sheet declaration compiled, but the compact renders approximate its medium height and do not prove identical iOS presentation or rasterization. End-state captures move the real scroll view programmatically.
- All content and interactions are mock data. No networking or backend integration was added. Settings alerts only preview the selected label. Search operates on the fictional track library.
- The existing unused LiveZoneStore emits a Swift 6 actor-isolation warning while this sketch compiles in Swift 5 mode. SwiftPM cache access warnings and Xcode CoreSimulator/provisioning diagnostics did not prevent successful compilation. No signing or installation was tested.
- Repository changes remain under `ios/`. `crates/` and Cargo files were untouched. No files were deleted, committed, or pushed. `git diff --check` passed; because `ios/` remains untracked, source/report whitespace and em-dash checks were also run directly.
- `br` is unavailable. Shared-memory MCP access was denied by the session's approval policy; CLI recall returned no project matches and a read-only database for the global bank. Memory retention could not write its journal. The local report and temporary worklog retain task evidence.

Logs: `/private/tmp/frankensonos-ios-polish-{xcodegen,build,render}.log`. Worklog: `/private/tmp/frankensonos-ios-polish-worklog.md`.
