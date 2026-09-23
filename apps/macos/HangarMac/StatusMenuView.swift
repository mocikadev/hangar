import SwiftUI

struct StatusMenuView: View {
    @Bindable var model: HangarModel
    let lifecycle: AppLifecycleController

    @Environment(\.openWindow) private var openWindow

    var body: some View {
        if let current = model.snapshot.accounts.first(where: { $0.account.current }) {
            Label(current.account.email, systemImage: "person.crop.circle.fill")
        } else {
            Label("当前账号未知", systemImage: "person.crop.circle.badge.questionmark")
        }

        Divider()

        ForEach(model.displayedAccounts, id: \.account.id) { card in
            Button(
                menuTitle(card),
                systemImage: card.account.current ? "checkmark.circle.fill" : "person.crop.circle",
                action: { switchAccount(card.account.id) }
            )
            .disabled(model.isBusy || card.account.current || card.account.stale)
        }

        Divider()

        Button("刷新周额度", systemImage: "arrow.clockwise") {
            Task { await model.refresh(force: true) }
        }
        .disabled(model.isBusy)
        Button("显示 Hangar", systemImage: "macwindow", action: showMainWindow)
        Button("退出 Hangar", systemImage: "power", action: lifecycle.terminate)
            .keyboardShortcut("q")
    }

    private func menuTitle(_ card: AccountCardRecord) -> String {
        let weekly = card.account.stale
            ? "需重新登录"
            : card.quota?.weeklyRemaining.map { "周剩余 \($0)%" } ?? "周额度未知"
        let old = card.quotaFetchedAt != nil && !card.quotaIsFresh ? " · 旧数据" : ""
        let suggested = card.recommended ? " ★ 推荐" : ""
        return "\(card.account.email) · \(weekly)\(old)\(suggested)"
    }

    private func showMainWindow() {
        lifecycle.prepareToShowMainWindow()
        if !lifecycle.showExistingMainWindow() {
            openWindow(id: HangarMacApp.mainWindowID)
        }
        lifecycle.finishShowingMainWindow()
    }

    private func switchAccount(_ accountID: String) {
        Task {
            await model.switchAccount(accountID: accountID)
        }
    }
}
