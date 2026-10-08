import Foundation
import Combine

@MainActor
final class MockZoneStore: ZoneStore {
    @Published private(set) var zones: [AudioZone]
    @Published var selectedZoneID: UUID
    @Published var selectedTab: RemoteTab = .rooms
    @Published var isPlayerPresented = false
    @Published var isRoomsSheetPresented = false
    @Published var isQueuePresented = false
    @Published var isSleepPresented = false
    @Published var sleepMinutes: Int?
    private var sleepDeadline: Date?
    private var sleepZoneID: UUID?
    @Published var elapsed: Double = 83
    @Published var browseSource: MusicSource?
    @Published var groupZone: AudioZone?
    @Published var groupEditingRooms: Set<String> = []
    @Published private(set) var roomVolumes: [String: Double] = [:]
    let rooms = ["Kitchen", "Bedroom", "Living Room", "Dining Room", "Office", "Patio", "Bathroom", "Garage", "Movie Room"]
    let tracks = Track.library
    private var roomIDs: [String: UUID] = [:]

    init() {
        let kitchen = UUID()
        selectedZoneID = kitchen
        zones = []
        for (index, room) in rooms.enumerated() {
            let id = index == 0 ? kitchen : UUID()
            roomIDs[room] = id
            roomVolumes[room] = [0.34, 0.28, 0.22, 0.48, 0.27, 0.41, 0.19, 0.32, 0.38][index]
            if index == 1 { continue }
            zones.append(AudioZone(id: id, roomNames: index == 0 ? ["Kitchen", "Bedroom"] : [room], track: tracks[[0, 0, 4, 7, 1, 3, 10, 5, 11][index]], isPlaying: [0, 3, 4, 5, 8].contains(index), volume: roomVolumes[room]!))
        }
    }

    var selectedZone: AudioZone { zones.first(where: { $0.id == selectedZoneID }) ?? zones[0] }
    func zone(for room: String) -> AudioZone { zones.first(where: { $0.roomNames.contains(room) }) ?? selectedZone }
    func model(for room: String) -> String {
        switch room {
        case "Kitchen", "Office": "Sonos Five"
        case "Living Room": "Sonos Play:5"
        case "Patio": "Sonos Move"
        case "Garage": "Sonos Amp"
        case "Movie Room": "Sonos Arc · Soundbar"
        default: "Sonos One"
        }
    }

    func togglePlayback(for zoneID: UUID) {
        guard let index = zones.firstIndex(where: { $0.id == zoneID }) else { return }
        zones[index].isPlaying.toggle()
    }
    func pauseAll() { zones.indices.forEach { zones[$0].isPlaying = false } }
    func setSleepTimer(_ minutes: Int?, now: Date = Date()) {
        sleepMinutes = minutes
        sleepDeadline = minutes.map { now.addingTimeInterval(Double($0 * 60)) }
        sleepZoneID = minutes == nil ? nil : selectedZoneID
    }
    func tick(now: Date = Date()) {
        if selectedZone.isPlaying { elapsed = min(selectedZone.track.duration, elapsed + 1) }
        guard let sleepDeadline, now >= sleepDeadline else { return }
        if let index = zones.firstIndex(where: { $0.id == sleepZoneID }) { zones[index].isPlaying = false }
        setSleepTimer(nil)
    }
    func setVolume(_ value: Double, for zoneID: UUID) {
        guard let index = zones.firstIndex(where: { $0.id == zoneID }) else { return }
        let level = min(1, max(0, value))
        zones[index].volume = level
        zones[index].roomNames.forEach { roomVolumes[$0] = level }
    }
    func setRoomVolume(_ value: Double, room: String) {
        roomVolumes[room] = min(1, max(0, value))
        guard let index = zones.firstIndex(where: { $0.roomNames.contains(room) }) else { return }
        zones[index].volume = zones[index].roomNames.map { roomVolumes[$0] ?? 0 }.reduce(0, +) / Double(zones[index].roomNames.count)
    }

    func setGroupedRooms(_ roomNames: [String], basedOn zoneID: UUID) {
        guard let source = zones.first(where: { $0.id == zoneID }) else { return }
        let selected = rooms.filter { roomNames.contains($0) }
        guard !selected.isEmpty else { return }
        let old = zones
        var reservedIDs = Set(old.map(\.id))
        var rebuilt = [AudioZone(id: zoneID, roomNames: selected, track: source.track, isPlaying: source.isPlaying, volume: averageVolume(selected))]
        for zone in old {
            let remaining = zone.roomNames.filter { !selected.contains($0) }
            guard !remaining.isEmpty else { continue }
            // Preserve unrelated groups; split only the group being edited.
            if zone.id == zoneID {
                for room in remaining {
                    let candidate = roomIDs[room] ?? UUID()
                    let id = reservedIDs.contains(candidate) ? UUID() : candidate
                    reservedIDs.insert(id)
                    rebuilt.append(AudioZone(id: id, roomNames: [room], track: zone.track, isPlaying: zone.isPlaying, volume: roomVolumes[room] ?? 0.3))
                }
            } else {
                rebuilt.append(AudioZone(id: zone.id, roomNames: remaining, track: zone.track, isPlaying: zone.isPlaying, volume: averageVolume(remaining)))
            }
        }
        zones = rebuilt.sorted { roomOrder($0) < roomOrder($1) }
        selectedZoneID = zoneID
    }
    private func roomOrder(_ zone: AudioZone) -> Int { rooms.firstIndex(of: zone.roomNames[0]) ?? 0 }
    private func averageVolume(_ names: [String]) -> Double { names.map { roomVolumes[$0] ?? 0.3 }.reduce(0, +) / Double(names.count) }
    func group(_ draggedID: UUID, onto targetID: UUID) {
        guard draggedID != targetID, let dragged = zones.first(where: { $0.id == draggedID }), let target = zones.first(where: { $0.id == targetID }) else { return }
        setGroupedRooms(target.roomNames + dragged.roomNames, basedOn: targetID)
    }
    func groupAll() { setGroupedRooms(rooms, basedOn: selectedZoneID) }
    func ungroupAll() {
        let old = zones
        let selectedRoom = selectedZone.roomNames[0]
        zones = rooms.map { room in
            let previous = old.first(where: { $0.roomNames.contains(room) })!
            return AudioZone(id: roomIDs[room]!, roomNames: [room], track: previous.track, isPlaying: previous.isPlaying, volume: roomVolumes[room] ?? 0.3)
        }
        selectedZoneID = roomIDs[selectedRoom]!
        groupZone = nil
    }
    func beginGroupEditing(_ zone: AudioZone) { groupZone = zone; groupEditingRooms = Set(zone.roomNames) }
    func toggleGroupRoom(_ room: String) {
        if groupEditingRooms.contains(room) { groupEditingRooms.remove(room) } else { groupEditingRooms.insert(room) }
    }
    func finishGroupEditing() {
        guard let groupZone, !groupEditingRooms.isEmpty else { return }
        setGroupedRooms(Array(groupEditingRooms), basedOn: groupZone.id)
        self.groupZone = nil
    }
    func selectTrack(_ track: Track, in zoneID: UUID) {
        guard let index = zones.firstIndex(where: { $0.id == zoneID }) else { return }
        zones[index].track = track
        zones[index].isPlaying = true
        elapsed = 0
    }
    func advanceTrack(in zoneID: UUID, direction: Int) {
        guard let zone = zones.first(where: { $0.id == zoneID }), let index = tracks.firstIndex(of: zone.track) else { return }
        selectTrack(tracks[(index + direction + tracks.count) % tracks.count], in: zoneID)
    }
}
