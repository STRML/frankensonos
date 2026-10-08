import Foundation
import Combine

/// The owner's Spotify library as the daemon serves it: status, albums, liked tracks and an album's tracks. The daemon
/// does the signing in and the reading; this model only asks, so it holds no credentials.
@MainActor
final class SpotifyModel: ObservableObject {
    enum Phase: Equatable {
        case loading
        case unreachable(String)
        case notConfigured
        case signedOut(reauthorize: Bool)
        case empty
        case ready
    }

    static let pageSize = 50

    @Published private(set) var status: SpotifyStatus?
    @Published private(set) var albums: [SpotifyAlbum] = []
    @Published private(set) var albumsTotal = 0
    @Published private(set) var liked: [SpotifyTrack] = []
    @Published private(set) var likedTotal = 0
    @Published private(set) var unreachable: String?
    @Published private(set) var error: String?
    private(set) var query = ""
    /// True for the canned library the screenshot tool shows; it never talks to a daemon.
    private(set) var isSample = false
    private var client: DaemonClient?
    private var albumTracks: [String: [SpotifyTrack]] = [:]
    private let pollInterval: Duration

    init(client: DaemonClient? = nil, pollInterval: Duration = .seconds(1)) {
        self.client = client
        self.pollInterval = pollInterval
    }

    /// Point the model at a daemon; the lists of any previous one are dropped.
    func attach(_ client: DaemonClient?) {
        guard self.client?.baseURL != client?.baseURL else { return }
        self.client = client
        status = nil
        albums = []; albumsTotal = 0; liked = []; likedTotal = 0
        albumTracks = [:]
        unreachable = nil; error = nil
    }

    var phase: Phase {
        let items = (status?.library.albums ?? 0) + (status?.library.tracks ?? 0)
        return Self.phase(status: status, unreachable: unreachable, albums: items)
    }

    /// `albums` is how many library items the daemon has cached (albums plus liked tracks).
    static func phase(status: SpotifyStatus?, unreachable: String?, albums: Int) -> Phase {
        if let unreachable { return .unreachable(unreachable) }
        guard let status else { return .loading }
        if !status.configured { return .notConfigured }
        if !status.signedIn { return .signedOut(reauthorize: status.reauthorize) }
        return albums == 0 ? .empty : .ready
    }

    func load() async {
        guard let client else { return }
        do {
            status = try await client.spotifyStatus()
            unreachable = nil
            if phase == .ready { await reloadLists() }
        } catch {
            unreachable = Self.describe(error)
            AppLog.shared.add("spotify", "status failed: \(Self.describe(error))")
        }
    }

    /// Ask the daemon to read the library, follow its progress, then show what it cached.
    func sync() async {
        guard let client else { return }
        error = nil
        do {
            try await client.spotifySync()
            AppLog.shared.add("spotify", "sync started")
            for _ in 0..<600 {
                status = try await client.spotifyStatus()
                if status?.sync.running == false { break }
                try await Task.sleep(for: pollInterval)
            }
            if let failure = status?.sync.error { error = failure.detail }
            AppLog.shared.add("spotify", "sync finished: \(status?.library.albums ?? 0) albums, \(status?.library.tracks ?? 0) liked tracks")
            await reloadLists()
        } catch is CancellationError {
            return
        } catch {
            self.error = Self.describe(error)
            AppLog.shared.add("spotify", "sync failed: \(Self.describe(error))")
        }
    }

    func search(_ text: String) async {
        query = text
        await reloadLists()
    }

    func tracks(for album: SpotifyAlbum) async -> [SpotifyTrack] {
        if let cached = albumTracks[album.id] { return cached }
        guard let client else { return [] }
        do {
            let fetched = try await client.spotifyAlbumTracks(id: album.id)
            albumTracks[album.id] = fetched
            return fetched
        } catch {
            self.error = Self.describe(error)
            return []
        }
    }

    func loadMoreAlbums() async {
        guard let client, albums.count < albumsTotal else { return }
        do {
            albums += try await client.spotifyAlbums(offset: albums.count, limit: Self.pageSize, query: query).items
        } catch { self.error = Self.describe(error) }
    }

    func loadMoreLiked() async {
        guard let client, liked.count < likedTotal else { return }
        do {
            liked += try await client.spotifyLiked(offset: liked.count, limit: Self.pageSize, query: query).items
        } catch { self.error = Self.describe(error) }
    }

    private func reloadLists() async {
        guard let client else { return }
        do {
            async let albumPage = client.spotifyAlbums(offset: 0, limit: Self.pageSize, query: query)
            async let likedPage = client.spotifyLiked(offset: 0, limit: Self.pageSize, query: query)
            let (a, l) = try await (albumPage, likedPage)
            (albums, albumsTotal) = (a.items, a.total)
            (liked, likedTotal) = (l.items, l.total)
        } catch {
            self.error = Self.describe(error)
            AppLog.shared.add("spotify", "library read failed: \(Self.describe(error))")
        }
    }

    #if DEBUG
    /// A signed-in library with a few albums, for the screenshot tool.
    static func sample() -> SpotifyModel {
        func album(_ id: String, _ title: String, _ artist: String, _ year: Int, _ tracks: Int) -> SpotifyAlbum {
            SpotifyAlbum(id: id, title: title, artist: artist, year: year, tracks: tracks, uri: "spotify:album:\(id)", artUrl: nil)
        }
        func track(_ id: String, _ title: String, _ artists: [String], _ album: String) -> SpotifyTrack {
            SpotifyTrack(id: id, title: title, artists: artists, uri: "spotify:track:\(id)", durationSecs: 214, disc: 1, number: 1, album: album, artUrl: nil)
        }
        let model = SpotifyModel()
        model.isSample = true
        model.status = SpotifyStatus(
            configured: true, signedIn: true, reauthorize: false,
            library: SpotifyLibrary(albums: 214, tracks: 2051, syncedAt: Int64(Date().timeIntervalSince1970 - 5400)),
            sync: SpotifySync(running: false, done: 0, total: 0, error: nil))
        model.albums = [
            album("a1", "Bach: Goldberg Variations, BWV 988", "Glenn Gould", 1981, 32),
            album("a2", "Beethoven: Symphonies Nos. 5 and 7", "Carlos Kleiber", 1976, 8),
            album("a3", "Kind of Blue", "Miles Davis", 1959, 5),
            album("a4", "Debussy: Preludes, Book 1", "Krystian Zimerman", 1994, 12),
            album("a5", "Mahler: Symphony No. 2", "Leonard Bernstein", 1987, 5),
            album("a6", "Satie: Gymnopédies and Gnossiennes", "Reinbert de Leeuw", 1977, 9)
        ]
        model.albumsTotal = 214
        model.liked = [
            track("t1", "Aria", ["Glenn Gould"], "Bach: Goldberg Variations, BWV 988"),
            track("t2", "Clair de lune", ["Krystian Zimerman"], "Debussy: Preludes, Book 1"),
            track("t3", "Blue in Green", ["Miles Davis", "Bill Evans"], "Kind of Blue")
        ]
        model.likedTotal = 2051
        return model
    }
    #endif

    /// The daemon's own explanation when it gave one (a policy denial, say), else the system's.
    private static func describe(_ error: Error) -> String {
        if let failure = error as? DaemonFailure { return failure.errorDescription ?? failure.detail }
        return error.localizedDescription
    }
}
