import Observation
import SwiftUI

@MainActor
@Observable
final class HangarModel {
    private let service = HangarService()
    private var loginPollTask: Task<Void, Never>?
    private(set) var snapshot: RefreshSnapshot
    private(set) var displayedAccounts: [AccountCardRecord]
    private(set) var switchingAccountID: String?
    private(set) var deletingAccountID: String?
    private(set) var isRunningDoctor = false
    var oauthFlow: OAuthFlowState?
    var doctorPresentation: DoctorPresentation?
    var notice: OperationNotice?

    init() {
        let initialSnapshot = service.refreshSnapshot()
        snapshot = initialSnapshot
        displayedAccounts = AccountDisplayOrder.sorted(initialSnapshot.accounts)
    }

    var isRefreshing: Bool {
        snapshot.phase == .pending || snapshot.phase == .running
    }

    var isBusy: Bool {
        isRefreshing || switchingAccountID != nil || deletingAccountID != nil || isRunningDoctor || oauthFlow != nil
    }

    func refresh(force: Bool = false) async {
        guard !isBusy else { return }
        let generation = force ? service.startRefreshAll() : service.startRefreshDue()
        guard generation != 0 else {
            return
        }
        repeat {
            updateSnapshot(service.refreshSnapshot())
            if snapshot.phase == .succeeded || snapshot.phase == .failed || snapshot.phase == .cancelled {
                return
            }
            try? await Task.sleep(for: .milliseconds(150))
        } while !Task.isCancelled
        _ = service.cancelRefresh(generation: generation)
    }

    private func updateSnapshot(_ next: RefreshSnapshot) {
        if next.phase == .pending || next.phase == .running {
            displayedAccounts = AccountDisplayOrder.preservingPositions(
                next.accounts,
                previous: displayedAccounts
            )
        } else {
            displayedAccounts = AccountDisplayOrder.sorted(next.accounts)
        }
        snapshot = next
    }

    func refreshAfterWake() async {
        guard !isBusy else {
            return
        }
        await refresh()
    }

    func switchAccount(accountID: String) async {
        guard !isBusy else {
            return
        }
        switchingAccountID = accountID
        defer { switchingAccountID = nil }

        do {
            let generation = try service.startSwitch(accountId: accountID)
            while !Task.isCancelled {
                let state = service.switchSnapshot()
                guard state.generation == generation else {
                    try? await Task.sleep(for: .milliseconds(100))
                    continue
                }
                switch state.phase {
                case .pending, .running:
                    try? await Task.sleep(for: .milliseconds(100))
                case .succeeded:
                    guard let result = state.result else {
                        notice = .error("切换已完成，但未返回账号信息。")
                        return
                    }
                    notice = .switchSucceeded(
                        email: result.email,
                        codexRunning: result.codexRunning
                    )
                    await refreshAfterSwitch()
                    return
                case .failed:
                    notice = .error(state.message ?? "账号切换失败。")
                    return
                case .cancelled:
                    notice = .error("账号切换已取消。")
                    return
                }
            }
        } catch {
            notice = .error(String(describing: error))
        }
    }

    func startLogin() -> URL? {
        startLogin(accountID: nil, email: nil)
    }

    func startReauth(accountID: String, email: String) -> URL? {
        startLogin(accountID: accountID, email: email)
    }

    func submitOAuthCallback(_ callbackURL: String) async {
        guard let flow = oauthFlow, flow.acceptsManualCallback else {
            return
        }
        let service = service
        do {
            try await Task.detached {
                try service.submitCallback(sessionId: flow.id, callbackUrl: callbackURL)
            }.value
            oauthFlow?.phase = .exchanging
            oauthFlow?.message = "正在交换登录凭据…"
        } catch {
            oauthFlow?.message = String(describing: error)
        }
    }

    func cancelLogin() {
        guard let sessionID = oauthFlow?.id else {
            return
        }
        loginPollTask?.cancel()
        _ = service.cancelLogin(sessionId: sessionID)
        _ = service.releaseLogin(sessionId: sessionID)
        oauthFlow = nil
    }

    func dismissLogin() {
        guard oauthFlow != nil else {
            return
        }
        cancelLogin()
    }

    func deleteAccount(accountID: String, email: String) async {
        guard !isBusy else {
            return
        }
        deletingAccountID = accountID
        defer { deletingAccountID = nil }
        let service = service
        do {
            try await Task.detached {
                try service.deleteAccount(accountId: accountID)
            }.value
            notice = .deleteSucceeded(email: email)
            deletingAccountID = nil
            await refresh()
        } catch {
            notice = .error(String(describing: error))
        }
    }

    func runDoctor() async {
        guard !isRunningDoctor else {
            return
        }
        isRunningDoctor = true
        defer { isRunningDoctor = false }
        let service = service
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString")
            as? String ?? "development"
        do {
            let report = try await Task.detached {
                try service.doctor(binaryVersion: version)
            }.value
            doctorPresentation = DoctorPresentation(
                lines: report.lines,
                issueCount: report.issueCount
            )
        } catch {
            notice = .error(String(describing: error))
        }
    }

    private func startLogin(accountID: String?, email: String?) -> URL? {
        guard !isBusy else {
            return nil
        }
        do {
            let start = if let accountID {
                try service.startReauth(accountId: accountID)
            } else {
                try service.startLogin()
            }
            guard let authorizationURL = URL(string: start.authorizationUrl) else {
                _ = service.cancelLogin(sessionId: start.sessionId)
                _ = service.releaseLogin(sessionId: start.sessionId)
                notice = .error("OAuth 授权地址无效。")
                return nil
            }
            let title = email.map { "重新登录 · \($0)" } ?? "添加账号"
            let successMessage = email.map {
                "已重新登录 \($0)；当前使用中的账号没有改变。"
            } ?? "账号已添加到 Hangar；当前使用中的账号没有改变。"
            oauthFlow = OAuthFlowState(
                id: start.sessionId,
                authorizationURL: authorizationURL,
                title: title,
                successMessage: successMessage
            )
            loginPollTask?.cancel()
            loginPollTask = Task { [weak self] in
                await self?.pollLogin(sessionID: start.sessionId)
            }
            return authorizationURL
        } catch {
            notice = .error(String(describing: error))
            return nil
        }
    }

    private func pollLogin(sessionID: UInt64) async {
        let service = service
        while !Task.isCancelled {
            do {
                let state = try await Task.detached {
                    try service.pollLogin(sessionId: sessionID)
                }.value
                guard oauthFlow?.id == sessionID else {
                    return
                }
                oauthFlow?.phase = state.phase
                oauthFlow?.message = state.message
                switch state.phase {
                case .waiting, .exchanging:
                    try? await Task.sleep(for: .milliseconds(150))
                case .succeeded:
                    let successMessage = oauthFlow?.successMessage ?? "登录成功。"
                    _ = service.releaseLogin(sessionId: sessionID)
                    oauthFlow = nil
                    notice = .loginSucceeded(successMessage)
                    await refresh(force: true)
                    return
                case .failed, .cancelled, .timedOut:
                    _ = service.releaseLogin(sessionId: sessionID)
                    if oauthFlow?.message == nil {
                        oauthFlow?.message = loginTerminalMessage(state.phase)
                    }
                    return
                }
            } catch {
                guard oauthFlow?.id == sessionID else {
                    return
                }
                _ = service.releaseLogin(sessionId: sessionID)
                oauthFlow?.phase = .failed
                oauthFlow?.message = String(describing: error)
                return
            }
        }
    }

    private func loginTerminalMessage(_ phase: LoginPhase) -> String {
        switch phase {
        case .cancelled:
            "登录已取消。"
        case .timedOut:
            "登录已超时，请关闭后重试。"
        case .failed:
            "登录失败，请关闭后重试。"
        case .waiting, .exchanging, .succeeded:
            ""
        }
    }

    private func refreshAfterSwitch() async {
        switchingAccountID = nil
        await refresh()
    }
}
