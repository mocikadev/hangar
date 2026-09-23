import SwiftUI

struct OAuthLoginSheet: View {
    @Environment(\.openURL) private var openURL
    @Bindable var model: HangarModel
    @State private var callbackURL = ""

    var body: some View {
        Group {
            if let flow = model.oauthFlow {
                VStack(alignment: .leading, spacing: 18) {
                    HStack {
                        Image(systemName: phaseSymbol(flow.phase))
                            .font(.title2)
                            .foregroundStyle(phaseColor(flow.phase))
                            .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 3) {
                            Text(flow.title)
                                .font(.title2.bold())
                            Text(phaseTitle(flow.phase))
                                .foregroundStyle(.secondary)
                        }
                    }

                    Text("浏览器会完成授权并自动返回 Hangar。若浏览器未自动回调，可粘贴完整回调地址。")
                        .foregroundStyle(.secondary)

                    Button("在浏览器中打开", systemImage: "safari") {
                        openURL(flow.authorizationURL)
                    }

                    TextField("http://localhost:1455/auth/callback?code=…", text: $callbackURL)
                        .textFieldStyle(.roundedBorder)
                        .disabled(!flow.acceptsManualCallback)
                        .accessibilityLabel("完整回调地址")

                    HStack {
                        Button("提交回调") {
                            Task {
                                await model.submitOAuthCallback(callbackURL)
                            }
                        }
                        .disabled(callbackURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !flow.acceptsManualCallback)
                        .keyboardShortcut(.defaultAction)

                        Spacer()

                        Button(flow.acceptsManualCallback || flow.phase == .exchanging ? "取消" : "关闭") {
                            model.cancelLogin()
                        }
                        .keyboardShortcut(.cancelAction)
                    }

                    if let message = flow.message, !message.isEmpty {
                        Text(message)
                            .foregroundStyle(flow.phase == .failed || flow.phase == .timedOut ? .red : .secondary)
                            .textSelection(.enabled)
                    }
                }
                .padding(24)
            } else {
                ContentUnavailableView("登录会话已结束", systemImage: "person.crop.circle.badge.checkmark")
            }
        }
        .frame(minWidth: 480, idealWidth: 520, minHeight: 310)
        .onExitCommand {
            model.cancelLogin()
        }
        .onDisappear {
            model.dismissLogin()
        }
    }

    private func phaseTitle(_ phase: LoginPhase) -> String {
        switch phase {
        case .waiting: "等待浏览器授权"
        case .exchanging: "正在交换登录凭据"
        case .succeeded: "登录成功"
        case .failed: "登录失败"
        case .cancelled: "登录已取消"
        case .timedOut: "登录已超时"
        }
    }

    private func phaseSymbol(_ phase: LoginPhase) -> String {
        switch phase {
        case .waiting: "person.badge.clock"
        case .exchanging: "arrow.trianglehead.2.clockwise.rotate.90"
        case .succeeded: "checkmark.circle.fill"
        case .failed, .timedOut: "exclamationmark.triangle.fill"
        case .cancelled: "xmark.circle.fill"
        }
    }

    private func phaseColor(_ phase: LoginPhase) -> Color {
        switch phase {
        case .succeeded: .green
        case .failed, .timedOut: .red
        case .cancelled: .secondary
        case .waiting, .exchanging: .accentColor
        }
    }
}
