import SwiftUI

@main
struct HangarMacApp: App {
    static let mainWindowID = "main"

    @NSApplicationDelegateAdaptor(AppLifecycleController.self) private var lifecycle

    var body: some Scene {
        WindowGroup("Hangar", id: Self.mainWindowID) {
            ContentView(model: lifecycle.model)
                .frame(minWidth: 480, minHeight: 420)
        }
        .defaultSize(width: 1180, height: 760)
        .commands {
            CommandGroup(replacing: .newItem) {}
        }

        MenuBarExtra {
            StatusMenuView(model: lifecycle.model, lifecycle: lifecycle)
        } label: {
            Image("HangarMenuIcon")
                .renderingMode(.template)
                .accessibilityLabel("Hangar")
        }
        .menuBarExtraStyle(.menu)
    }
}
