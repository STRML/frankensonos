import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var store: MockZoneStore
    @State private var daemonURL = DaemonSettings.urlString
    @State private var invalidURL = false
    @State private var detail: String?
    private let sections: [(String, [(String, String, String)])] = [
        ("System", [("house", "My System", "9 speakers"), ("slider.horizontal.3", "Room Settings", ""), ("alarm", "Alarms", "None set")]),
        ("Services and Voice", [("music.note", "Music Services", "3 services"), ("mic", "Voice Assistants", "Not set up")]),
        ("Account", [("person.crop.circle", "Your Account", "Demo account")]),
        ("Help", [("questionmark.circle", "Help & Tips", ""), ("info.circle", "About My System", "FrankenSonos")])
    ]
    private var daemonSettings: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("DAEMON").s1Font(11, weight: .semibold).foregroundStyle(.secondary)
            TextField("Daemon URL", text: $daemonURL).textFieldStyle(.plain).s1Font(14)
                .onSubmit(saveURL).accessibilityLabel("Daemon URL")
                #if os(iOS)
                .keyboardType(.URL).textInputAutocapitalization(.never).autocorrectionDisabled()
                #endif
            HStack {
                Text("Connection").s1Font(14)
                Spacer()
                Text(store.connectionStatus.rawValue.capitalized).s1Font(12).foregroundStyle(.secondary)
            }
            HStack {
                Text(URL(string: store.daemonURLString)?.host ?? store.daemonURLString).s1Font(11).foregroundStyle(.secondary)
                Spacer()
                Button("Connect", action: saveURL).s1Font(13, weight: .semibold)
            }
            if invalidURL { Text("Enter an http:// or https:// daemon URL.").s1Font(12).foregroundStyle(.red) }
        }
        .padding(16).background(.white)
    }
    private func saveURL() { invalidURL = !store.changeDaemonURL(daemonURL) }
    var body: some View {
        GeometryReader { geometry in
            ScrollView(.vertical, showsIndicators: false) {
                VStack(alignment: .leading, spacing: 0) {
                    if store.isLive { daemonSettings }
                    ForEach(store.isLive ? [] : sections, id: \.0) { section in
                        Text(section.0.uppercased()).s1Font(11, weight: .semibold)
                            .foregroundStyle(Color(white: 0.38))
                            .padding(.horizontal, 16).padding(.top, 22).padding(.bottom, 8)
                        VStack(spacing: 0) {
                            ForEach(section.1, id: \.1) { row in
                                Button { detail = row.1 } label: {
                                    HStack(spacing: 12) {
                                        Image(systemName: row.0).font(.system(size: 19, weight: .light)).frame(width: 24)
                                        Text(row.1).s1Font(14)
                                        Spacer(minLength: 4)
                                        Text(row.2).s1Font(11).foregroundStyle(Color(white: 0.38)).lineLimit(1)
                                        Image(systemName: "chevron.right").font(.system(size: 11)).foregroundStyle(Color(white: 0.38))
                                    }
                                    .padding(.horizontal, 16).frame(minHeight: 50)
                                    .background(.white)
                                    .overlay(alignment: .bottom) { Rectangle().fill(Color.black.opacity(0.08)).frame(height: 0.5).padding(.leading, 52) }
                                    .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                    Text(store.isLive ? "FrankenSonos · Daemon remote" : "FrankenSonos · Mock remote").s1Font(11).foregroundStyle(Color(white: 0.38))
                        .frame(maxWidth: .infinity).padding(.vertical, 24)
                }
                .frame(width: geometry.size.width, alignment: .leading)
                .padding(.bottom, 16)
            }
            .scrollClipDisabled()
        }
        .background(Color(white: 0.95))
        .alert(detail ?? "Settings", isPresented: Binding(get: { detail != nil }, set: { if !$0 { detail = nil } })) {
            Button("OK") { detail = nil }
        } message: { Text("This setting is a local mock preview.") }
    }
}
