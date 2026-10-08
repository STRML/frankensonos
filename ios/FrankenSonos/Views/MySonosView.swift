import SwiftUI

struct MySonosView: View {
    @EnvironmentObject private var store: MockZoneStore
    var openPlayer: () -> Void = {}
    var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 0) {
                Text("Play on \(store.selectedZone.displayName)").s1Font(11).foregroundStyle(.secondary).padding(.horizontal, 16).frame(height: 32)
                HStack(spacing: 8) {
                    Text("Recently Played").s1Font(18, weight: .semibold)
                    Image(systemName: "chevron.right").font(.system(size: 12, weight: .semibold))
                    Spacer()
                }
                .padding(.horizontal, 16).padding(.top, 8).padding(.bottom, 16)
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(alignment: .top, spacing: 12) {
                        ForEach(Array(store.tracks.prefix(6))) { track in
                            Button { play(track) } label: {
                                VStack(alignment: .leading, spacing: 7) {
                                    AlbumArtworkView(track: track).frame(width: 136, height: 136)
                                    Text(track.album).s1Font(13, weight: .medium).lineLimit(1)
                                    Text(track.artist).s1Font(11).foregroundStyle(.secondary).lineLimit(1)
                                }
                                .frame(width: 136, alignment: .leading)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 16).padding(.bottom, 8)
                }
                HStack(spacing: 8) {
                    Text("Songs").s1Font(18, weight: .semibold)
                    Image(systemName: "chevron.right").font(.system(size: 12, weight: .semibold))
                    Spacer()
                    Image(systemName: "star").font(.system(size: 17)).foregroundStyle(.secondary)
                }
                .padding(.horizontal, 16).padding(.top, 16).padding(.bottom, 12)
                ForEach(store.tracks) { track in SongRow(track: track) { play(track) } }
            }
            .padding(.bottom, 16)
        }
    }
    private func play(_ track: Track) {
        store.selectTrack(track, in: store.selectedZoneID)
        openPlayer()
    }
}
