import AppKit

@MainActor
final class AppLifecycleController: NSObject, NSApplicationDelegate {
    let model = HangarModel()
    private var quotaRefreshTask: Task<Void, Never>?

    func applicationDidFinishLaunching(_ notification: Notification) {
        if let icon = NSImage(named: "HangarDockIcon") {
            NSApp.applicationIconImage = icon
        }
        Task { await model.refresh() }
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(windowWillClose(_:)),
            name: NSWindow.willCloseNotification,
            object: nil
        )
        NSWorkspace.shared.notificationCenter.addObserver(
            self,
            selector: #selector(workspaceDidWake(_:)),
            name: NSWorkspace.didWakeNotification,
            object: nil
        )
        quotaRefreshTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(60))
                guard !Task.isCancelled else { return }
                await self?.model.refreshAfterWake()
            }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        quotaRefreshTask?.cancel()
        NotificationCenter.default.removeObserver(self)
        NSWorkspace.shared.notificationCenter.removeObserver(self)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldSaveApplicationState(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldRestoreApplicationState(_ sender: NSApplication) -> Bool {
        false
    }

    func prepareToShowMainWindow() {
        NSApp.setActivationPolicy(.regular)
    }

    func finishShowingMainWindow() {
        Task { @MainActor in
            await Task.yield()
            NSApp.activate(ignoringOtherApps: true)
            NSApp.windows.first(where: isMainWindow)?.makeKeyAndOrderFront(nil)
        }
    }

    func showExistingMainWindow() -> Bool {
        guard let window = NSApp.windows.first(where: isMainWindow) else {
            return false
        }
        window.deminiaturize(nil)
        window.makeKeyAndOrderFront(nil)
        return true
    }

    func terminate() {
        NSApp.terminate(nil)
    }

    @objc private func windowWillClose(_ notification: Notification) {
        guard let window = notification.object as? NSWindow,
              window.canBecomeMain
        else {
            return
        }
        guard !NSApp.windows.contains(where: { candidate in
            candidate !== window && isMainWindow(candidate)
        }) else {
            return
        }
        NSApp.setActivationPolicy(.accessory)
    }

    @objc private func workspaceDidWake(_ notification: Notification) {
        Task {
            await model.refreshAfterWake()
        }
    }

    private func isMainWindow(_ window: NSWindow) -> Bool {
        window.isVisible && window.canBecomeMain
    }
}
