import SwiftUI

#if os(iOS)
@main
struct FrankenSonosApp: App {
    @StateObject private var store = MockZoneStore()
    var body: some Scene {
        WindowGroup {
            RemoteRoot()
                .environmentObject(store)
                .tint(.primary)
                .background(Color(white: 0.025).ignoresSafeArea(edges: .bottom))
                .preferredColorScheme(store.isPlayerPresented ? .dark : nil)
        }
    }
}
#endif
