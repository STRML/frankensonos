import Foundation

extension LiveZoneStore {
    func togglePlayback(for zoneID: UUID) {
        guard let index = zones.firstIndex(where: { $0.id == zoneID }), let room = coordinator(zoneID), !offlineRooms.contains(room) else { return }
        let original = zones[index].isPlaying
        let desired = !original
        zones[index].isPlaying = desired
        send(desired ? "resume" : "pause", room: room) { [weak self] in
            guard let self, let index = self.zones.firstIndex(where: { $0.id == zoneID }) else { return }
            self.zones[index].isPlaying = original
        }
    }
    func pauseAll() {
        for zone in zones where zone.isPlaying { togglePlayback(for: zone.id) }
    }
    func advanceTrack(in zoneID: UUID, direction: Int) {
        guard let room = coordinator(zoneID) else { return }
        send(direction < 0 ? "previous" : "next", room: room)
    }
    func selectTrack(_ track: Track, in zoneID: UUID) {
        guard let index = zones.firstIndex(where: { $0.id == zoneID }), let room = coordinator(zoneID), let favorite = track.favoriteID else { return }
        guard let row = roomRows.first(where: { roomKey($0) == room }), row.household == track.household else {
            commandError = "Choose a favorite from this room's household."
            return
        }
        let original = zones[index]
        zones[index].track = track
        zones[index].isPlaying = true
        positions[zoneID] = (0, Date())
        elapsed = 0
        send("play/favorite", room: room, extra: ["favorite": favorite]) { [weak self] in
            guard let self, let index = self.zones.firstIndex(where: { $0.id == zoneID }) else { return }
            self.zones[index] = original
        }
    }

    private func send(_ path: String, room: String, extra: [String: Any] = [:], revert: @escaping () -> Void = {}) {
        guard !offlineRooms.contains(room) else { revert(); return }
        let id = UUID()
        commandTasks[id] = Task { [weak self] in
            guard let self else { return }
            defer { self.commandTasks[id] = nil }
            do {
                var body = extra
                body["zone"] = self.target(room)
                try await self.client.command(path, body: body)
            } catch {
                guard !Task.isCancelled else { return }
                revert()
                self.showFailure(error)
                if ((error as? DaemonFailure)?.status ?? 500) >= 500 { try? await self.refetch(room: room) }
                return
            }
            do { try await self.refetch(room: room) }
            catch { if !Task.isCancelled { self.scheduleState(room: room) } }
        }
    }

    func setGroupedRooms(_ roomNames: [String], basedOn zoneID: UUID) {
        if grouping {
            queuedGrouping = (roomNames, zoneID)
            return
        }
        guard let source = zones.first(where: { $0.id == zoneID }),
              let coordinator = source.roomNames.first, !roomNames.isEmpty else { return }
        let selected = Array(Set(roomNames)).sorted()
        let households = Set(selected.compactMap { name in roomRows.first(where: { roomKey($0) == name })?.household })
        guard households.count <= 1 else {
            commandError = "Rooms from different households cannot share a group."
            return
        }
        let leader = selected.contains(coordinator) ? coordinator : selected[0]
        let oldZones = zones
        let oldSelection = selectedZoneID
        grouping = true
        topologyTask?.cancel()
        topologyTask = nil
        optimisticGroup(selected, leader: leader, source: source)
        let id = UUID()
        let revision = lifecycleRevision
        commandTasks[id] = Task { [weak self] in
            guard let self else { return }
            defer {
                self.commandTasks[id] = nil
                if self.lifecycleRevision == revision {
                    self.grouping = false
                    if let next = self.queuedGrouping {
                        self.queuedGrouping = nil
                        self.setGroupedRooms(next.rooms, basedOn: next.zoneID)
                    }
                }
            }
            do {
                for room in source.roomNames where !selected.contains(room) {
                    try await self.client.command("ungroup", body: ["zone": self.target(room)])
                }
                if leader != coordinator { try await self.client.command("ungroup", body: ["zone": self.target(leader)]) }
                for room in selected where room != leader {
                    try await self.client.command("group", body: ["zone": self.target(room), "to": self.target(leader)])
                }
                try await self.refreshAll(afterGrouping: true)
            } catch {
                guard !Task.isCancelled else { return }
                self.zones = oldZones
                self.selectedZoneID = oldSelection
                self.showFailure(error)
                try? await self.refreshAll(afterGrouping: true)
            }
        }
    }

    func groupAll() {
        guard let room = coordinator(selectedZoneID), let household = roomRows.first(where: { roomKey($0) == room })?.household else { return }
        setGroupedRooms(roomRows.filter { $0.household == household }.map(roomKey), basedOn: selectedZoneID)
    }

    func ungroupAll() {
        guard !grouping else { return }
        let oldZones = zones
        grouping = true
        topologyTask?.cancel()
        topologyTask = nil
        zones = oldZones.flatMap { zone in
            zone.roomNames.map { name in
                let row = roomRows.first(where: { roomKey($0) == name })
                let id = StableIdentity.zone(room: row?.name ?? name, household: row?.household ?? "")
                return AudioZone(id: id, roomNames: [name], track: zone.track, isPlaying: zone.isPlaying, volume: roomVolumes[name] ?? 0)
            }
        }
        let roomsToDetach = oldZones.flatMap { Array($0.roomNames.dropFirst()) }
        let id = UUID()
        let revision = lifecycleRevision
        commandTasks[id] = Task { [weak self] in
            guard let self else { return }
            defer {
                self.commandTasks[id] = nil
                if self.lifecycleRevision == revision { self.grouping = false }
            }
            do {
                for room in roomsToDetach { try await self.client.command("ungroup", body: ["zone": self.target(room)]) }
                try await self.refreshAll(afterGrouping: true)
            } catch {
                guard !Task.isCancelled else { return }
                self.zones = oldZones
                self.showFailure(error)
                try? await self.refreshAll(afterGrouping: true)
            }
        }
    }

    private func optimisticGroup(_ selected: [String], leader: String, source: AudioZone) {
        let household = roomRows.first(where: { roomKey($0) == leader })?.household ?? ""
        let name = roomRows.first(where: { roomKey($0) == leader })?.name ?? leader
        let id = StableIdentity.zone(room: name, household: household)
        let members = [leader] + selected.filter { $0 != leader }
        var result = [AudioZone(id: id, roomNames: members, track: source.track, isPlaying: source.isPlaying, volume: source.volume)]
        for zone in zones {
            let remaining = zone.roomNames.filter { !selected.contains($0) }
            if zone.id == source.id {
                for room in remaining {
                    let row = roomRows.first(where: { roomKey($0) == room })
                    let roomID = StableIdentity.zone(room: row?.name ?? room, household: row?.household ?? "")
                    result.append(AudioZone(id: roomID, roomNames: [room], track: zone.track, isPlaying: zone.isPlaying, volume: roomVolumes[room] ?? 0))
                }
            } else if !remaining.isEmpty {
                var kept = zone
                kept.roomNames = remaining
                result.append(kept)
            }
        }
        zones = result
        selectedZoneID = id
    }
}
