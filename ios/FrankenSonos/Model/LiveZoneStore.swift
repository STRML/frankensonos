import Foundation
import Combine

/// Placeholder for a future client of the FrankenSonos daemon HTTP API.
/// It would read zones, room state, favorites, and events, then send controls.
/// This sketch intentionally contains no networking implementation.
@MainActor
final class LiveZoneStore: ZoneStore {
    let objectWillChange = ObservableObjectPublisher()
    var zones: [AudioZone] { [] }
    var selectedZoneID = UUID()
    var rooms: [String] { [] }
    var tracks: [Track] { [] }

    func togglePlayback(for zoneID: UUID) {}
    func pauseAll() {}
    func setVolume(_ value: Double, for zoneID: UUID) {}
    func setGroupedRooms(_ roomNames: [String], basedOn zoneID: UUID) {}
    func selectTrack(_ track: Track, in zoneID: UUID) {}
    func advanceTrack(in zoneID: UUID, direction: Int) {}
}
