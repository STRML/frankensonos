import Foundation
import XCTest
@testable import LiveModel

@MainActor
final class LiveChecks: XCTestCase {
    static var failures = 0
    static let env = ProcessInfo.processInfo.environment
    static func check(_ row: Int, _ description: String, _ run: () async throws -> Void) async {
        do { try await run(); print("PASS row \(row): \(description)") }
        catch {
            failures += 1
            print("FAIL row \(row): \(description): \(error)")
            XCTFail("row \(row): \(error)")
        }
    }
    static func require(_ condition: Bool, _ message: String) throws {
        if !condition { throw CheckFailure(message: message) }
    }
    static func wait(_ message: String, seconds: Double = 12, _ condition: @escaping () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(seconds)
        while Date() < deadline {
            if condition() { return }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw CheckFailure(message: message)
    }
    static func control(_ command: String) async throws {
        let path = env["FSONOS_E2E_CONTROL"]!
        try command.write(toFile: path, atomically: true, encoding: .utf8)
        try await wait("daemon \(command) handshake") {
            (try? String(contentsOfFile: path, encoding: .utf8)) == (command == "stop" ? "stopped\n" : "started\n")
        }
    }
    static func fixture(_ name: String) throws -> Data {
        try Data(contentsOf: Bundle.module.url(forResource: name, withExtension: nil, subdirectory: "Fixtures")!)
    }
    func testFailureMatrix() async throws { try await Self.runMatrix() }
    static func runMatrix() async throws {
        let url = URL(string: env["FSONOS_DAEMON_URL"]!)!
        let client = DaemonClient(baseURL: url)
        let store = LiveZoneStore(baseURL: url)
        await check(19, "bonjour finds an advertised daemon and resolves it to a URL") {
            let name = env["FSONOS_E2E_BONJOUR_NAME"]!
            let port = Int(env["FSONOS_E2E_BONJOUR_PORT"]!)!
            let browser = DaemonBrowser()
            browser.start()
            defer { browser.stop() }
            do {
                try await wait("bonjour discovery", seconds: 20) { browser.found.contains { $0.name == name } }
            } catch {
                throw CheckFailure(message: "bonjour discovery of '\(name)': saw \(browser.found.map(\.name))")
            }
            let hit = browser.found.first { $0.name == name }!
            try require(hit.url.port == port, "port was lost: \(hit.url)")
            // IPv4 when the registering host has one, else its .local name (this Mac publishes only IPv6 link-local).
            try require(hit.url.host.map { !$0.isEmpty } == true, "no host in \(hit.url)")
        }
        await check(1, "unreachable launch stays empty/offline, then bootstraps") {
            try await control("stop")
            store.start()
            try await wait("offline launch") { store.connectionStatus == .offline }
            try require(store.zones.isEmpty, "offline launch showed sample rooms")
            try await control("start")
            try await wait("live bootstrap", seconds: 25) { store.connectionStatus == .live && store.zones.count == 2 }
            try require(Set(store.rooms) == ["Bedroom", "Living Room"], "incorrect inventory")
        }
        guard let id = store.zones.first(where: { $0.roomNames.first == "Living Room" })?.id else {
            XCTFail("missing bootstrap zone"); return
        }
        await check(10, "favorite playback and external track event refetch metadata") {
            try await wait("favorites") { !store.tracks.isEmpty }
            store.selectTrack(store.tracks.first(where: { $0.title == "Nocturne in E-flat" })!, in: id)
            try await wait("favorite state") { store.zones.first(where: { $0.id == id })?.track.title == "Nocturne in E-flat" }
            try await waitForTrack("Nocturne in E-flat", client: client)
            try await client.command("play/favorite", body: ["zone": "Living Room", "favorite": "FV:2/1"])
            try await wait("SSE track metadata") { store.zones.first(where: { $0.id == id })?.track.title == "Aria" }
        }
        await check(8, "volume drag is immediate, debounced and last write wins") {
            for value in [0.31, 0.32, 0.33, 0.34, 0.35] { store.setVolume(value, for: id) }
            try require(store.zones.first(where: { $0.id == id })?.volume == 0.35, "volume was not optimistic")
            try await Task.sleep(for: .seconds(1))
            let state = try await client.state(room: "Living Room")
            try require(state.volume == 35, "last volume did not reach speaker: \(String(describing: state.volume))")
            try require(store.roomVolumes["Living Room"] == 0.35, "echo overwrote slider")
        }
        await check(6, "bad room command reverts optimistic volume with daemon detail") {
            let before = store.roomVolumes["Missing Room"]
            store.setRoomVolume(0.61, room: "Missing Room")
            try require(store.roomVolumes["Missing Room"] == 0.61, "bad-room volume was not optimistic")
            try await wait("4xx toast") { store.commandError != nil }
            try require(store.roomVolumes["Missing Room"] == before, "4xx left optimistic volume behind")
            try require(store.commandError!.contains("Missing Room"), "daemon detail was lost")
        }
        await check(9, "group/ungroup topology stays idempotent") {
            store.setGroupedRooms(["Living Room", "Bedroom"], basedOn: id)
            try await wait("group") { store.zones.count == 1 && store.zones[0].roomNames.count == 2 }
            try await Task.sleep(for: .seconds(1))
            let grouped = try await client.zones()
            try require(grouped.count == 1 && Set(grouped[0].members) == Set(store.zones[0].roomNames), "group differs from daemon")
            store.setGroupedRooms(["Living Room"], basedOn: id)
            try await wait("ungroup") { store.zones.count == 2 }
            try await Task.sleep(for: .seconds(1))
            try require(try await client.zones().count == 2, "ungroup differs from daemon")
            try require(store.zones.contains(where: { $0.id == id }), "coordinator ID changed")
        }
        await check(2, "SSE disconnect reconnects and observes later external commands") {
            try await wait("event cursor") { store.lastEventID != nil }
            try await control("stop")
            try await wait("reconnecting") { store.connectionStatus == .reconnecting }
            try await control("start")
            try await wait("reconnected", seconds: 25) { store.connectionStatus == .live }
            try await client.command("volume", body: ["zone": "Living Room", "volume": 26])
            try await wait("event after reconnect", seconds: 20) { store.roomVolumes["Living Room"] == 0.26 }
        }
        await check(7, "unreachable command reverts and reports failure") {
            try await control("stop")
            let before = store.roomVolumes["Living Room"]
            store.commandError = nil
            store.setRoomVolume(0.58, room: "Living Room")
            try await wait("network toast") { store.commandError != nil }
            try require(store.roomVolumes["Living Room"] == before, "network failure left optimistic volume")
            try require(store.commandError!.contains("Didn't go through"), "network failure message lost")
            try await control("start")
            try await wait("live again", seconds: 25) { store.connectionStatus == .live }
        }
        await check(17, "foreground fully refreshes after background cancellation") {
            store.setActive(false)
            try await client.command("volume", body: ["zone": "Living Room", "volume": 29])
            store.setActive(true)
            do {
                try await wait("foreground volume") { store.roomVolumes["Living Room"] == 0.29 && store.connectionStatus == .live }
            } catch {
                let seen = "volume=\(String(describing: store.roomVolumes["Living Room"])) status=\(store.connectionStatus)"
                throw CheckFailure(message: "foreground volume: \(seen)")
            }
        }
        await check(11, "captured streams and absent tracks have no scrubber duration") {
            let decoder = JSONDecoder()
            let now = try decoder.decode(ZoneStateDTO.self, from: fixture("state-now.json"))
            let stopped = try decoder.decode(ZoneStateDTO.self, from: fixture("state-stopped.json"))
            try require(now.track?.duration_secs == 0 && stopped.track == nil, "optional track/duration contract")
            let missing = try decoder.decode(TrackDTO.self, from: Data("{\"uri\":\"stream:placeholder\"}".utf8))
            try require(missing.duration_secs == nil && missing.title == nil, "missing metadata failed")
            try require(store.zones.first(where: { $0.id == id })!.track.duration == 0, "stream got a fictional duration")
        }
        await check(12, "playing position ticks locally and pauses") {
            let track = Track.live(title: "Placeholder", artist: "", album: "", key: "placeholder", duration: 120)
            try require(track.duration == 120, "duration mapping")
            let before = store.elapsed
            store.tick(now: Date().addingTimeInterval(3))
            try require(store.elapsed >= before, "position moved backward")
            store.togglePlayback(for: id)
            try await wait("paused") { !store.zones.first(where: { $0.id == id })!.isPlaying }
            try await waitForTransport("paused", client: client)
            store.togglePlayback(for: id)
            try await wait("resumed") { store.zones.first(where: { $0.id == id })!.isPlaying }
        }
        await check(13, "spaces, accents and plus use one encoded path segment") {
            let special = client.stateURL(room: "Pièce + Room")
            try require(special.absoluteString.contains("Pi%C3%A8ce%20%2B%20Room"), "wrong path: \(special)")
            try require(try await client.state(room: "Living Room").zone.coordinator_room == "Living Room", "space route failed")
        }
        await check(15, "fixture decoding tolerates unknown fields and SSE kinds") {
            try checkJSONFixtures()
            try checkSSEFixture()
            try await checkUnknownEvent(store)
        }
        store.setActive(false)
        print("Live checks: \(failures) failed")
    }

    static func checkJSONFixtures() throws {
        let decoder = JSONDecoder()
        let zonesData = try fixture("zones.json")
        let roomsData = try fixture("rooms.json")
        let favoritesData = try fixture("favorites.json")
        let zones = try decoder.decode([ZoneDTO].self, from: zonesData)
        let rooms = try decoder.decode([RoomDTO].self, from: roomsData)
        let favorites = try decoder.decode([FavoriteDTO].self, from: favoritesData)
        try require(zones.count == 2, "zone fixture count")
        try require(rooms.count == 2, "room fixture count")
        try require(favorites.count == 5, "favorite fixture count")
        let mutated = Data("{\"uri\":\"placeholder\",\"future\":{\"nested\":true}}".utf8)
        _ = try decoder.decode(TrackDTO.self, from: mutated)
    }
    static func waitForTrack(_ title: String, client: DaemonClient) async throws {
        let deadline = Date().addingTimeInterval(12)
        while Date() < deadline {
            let state = try await client.state(room: "Living Room")
            if state.track?.title == title { return }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw CheckFailure(message: "favorite command did not reach daemon: \(title)")
    }
    /// The store updates optimistically, so the daemon sees the command a moment later.
    static func waitForTransport(_ expected: String, client: DaemonClient) async throws {
        let deadline = Date().addingTimeInterval(12)
        while Date() < deadline {
            if try await client.state(room: "Living Room").transport_state == expected { return }
            try await Task.sleep(for: .milliseconds(50))
        }
        throw CheckFailure(message: "pause did not reach speaker")
    }
    static func checkSSEFixture() throws {
        let bytes = try fixture("events.sse")
        let lines = String(decoding: bytes, as: UTF8.self).components(separatedBy: "\n")
        var parser = SSEParser()
        var frames: [DaemonEvent] = []
        for line in lines {
            if let frame = parser.consume(line) { frames.append(frame) }
        }
        try require(frames.first?.id == "9", "first SSE cursor")
        try require(frames.last?.id == "20", "last SSE cursor")
    }
    static func checkUnknownEvent(_ store: LiveZoneStore) async throws {
        let count = store.zones.count
        let event = DaemonEvent(id: "future", kind: "future.kind", data: "{}")
        await store.apply(event)
        try require(store.zones.count == count, "unknown event changed inventory")
    }
}

struct CheckFailure: Error, CustomStringConvertible {
    let message: String
    var description: String { message }
}
