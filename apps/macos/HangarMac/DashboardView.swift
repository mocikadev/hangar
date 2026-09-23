import SwiftUI

struct DashboardView: View {
    @Environment(\.openURL) private var openURL
    @Bindable var model: HangarModel

    private let columns = [
        GridItem(.adaptive(minimum: 260, maximum: 360), spacing: 16, alignment: .top)
    ]

    var body: some View {
        NavigationStack {
            Group {
                if model.snapshot.accounts.isEmpty && !model.isRefreshing {
                    VStack(spacing: 12) {
                        ContentUnavailableView(
                            "暂无账号",
                            systemImage: "person.crop.circle.badge.questionmark",
                            description: Text("可在这里添加账号；已有 Codex 登录会自动识别。")
                        )
                        Button("添加账号", systemImage: "person.badge.plus", action: addAccount)
                            .disabled(model.isBusy)
                    }
                } else {
                    ScrollView {
                        LazyVGrid(columns: columns, alignment: .leading, spacing: 16) {
                            ForEach(model.displayedAccounts, id: \.account.id) { card in
                                AccountCardView(
                                    card: card,
                                    isSwitching: model.switchingAccountID == card.account.id,
                                    isDeleting: model.deletingAccountID == card.account.id,
                                    actionsDisabled: model.isBusy,
                                    switchAction: { switchAccount(card.account.id) },
                                    reauthAction: { reauthenticate(card.account.id, email: card.account.email) },
                                    deleteAction: { deleteAccount(card.account.id, email: card.account.email) }
                                )
                            }
                        }
                        .padding()
                    }
                }
            }
            .navigationTitle("账号总览")
            .safeAreaInset(edge: .top) {
                if ProcessInfo.processInfo.environment["HANGAR_TEST_HOME"] != nil {
                    Label("隔离验证模式 · 不会修改真实账号库", systemImage: "testtube.2")
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(.orange)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                        .background(.orange.opacity(0.12))
                        .accessibilityLabel("隔离验证模式，不会修改真实账号库")
                }
            }
            .toolbar {
                Button("添加账号", systemImage: "person.badge.plus", action: addAccount)
                    .disabled(model.isBusy)
                    .keyboardShortcut("n", modifiers: .command)
                    .help("添加账号（⌘N）")
                Button("自检", systemImage: "checkmark.shield", action: runDoctor)
                    .disabled(model.isBusy || model.isRunningDoctor)
                    .keyboardShortcut("d", modifiers: [.command, .shift])
                    .help("检查账号库、文件权限和 Codex 配置（⇧⌘D）")
                Button("刷新全部", systemImage: "arrow.clockwise", action: refresh)
                    .disabled(model.isBusy)
                    .keyboardShortcut("r", modifiers: .command)
                    .help("刷新全部账号的周剩余额度（⌘R）")
            }
            .sheet(item: $model.oauthFlow) { _ in
                OAuthLoginSheet(model: model)
            }
            .sheet(item: $model.doctorPresentation) { report in
                DoctorSheetView(report: report)
            }
            .alert(item: $model.notice) { notice in
                Alert(
                    title: Text(notice.title),
                    message: Text(notice.message),
                    dismissButton: .default(Text("好"))
                )
            }
        }
    }

    private func addAccount() {
        if let authorizationURL = model.startLogin() {
            openURL(authorizationURL)
        }
    }

    private func refresh() {
        Task {
            await model.refresh(force: true)
        }
    }

    private func switchAccount(_ accountID: String) {
        Task {
            await model.switchAccount(accountID: accountID)
        }
    }

    private func reauthenticate(_ accountID: String, email: String) {
        if let authorizationURL = model.startReauth(accountID: accountID, email: email) {
            openURL(authorizationURL)
        }
    }

    private func deleteAccount(_ accountID: String, email: String) {
        Task {
            await model.deleteAccount(accountID: accountID, email: email)
        }
    }

    private func runDoctor() {
        Task {
            await model.runDoctor()
        }
    }
}
