import SwiftUI

#if os(iOS)
@main
struct FrankenSonosApp: App {
    @Environment(\.scenePhase) private var scenePhase
    @StateObject private var store: MockZoneStore
    init() {
        _store = StateObject(wrappedValue: CommandLine.arguments.contains("-mock") ? MockZoneStore() : MockZoneStore(live: LiveZoneStore()))
    }
    var body: some Scene {
        WindowGroup {
            RemoteRoot()
                .environmentObject(store)
                .onAppear { store.start() }
                .onChange(of: scenePhase) { _, phase in store.setActive(phase == .active) }
                .tint(.primary)
                .background(Color(white: 0.025).ignoresSafeArea(edges: .bottom))
                .preferredColorScheme(store.isPlayerPresented ? .dark : nil)
        }
    }
}
#endif
