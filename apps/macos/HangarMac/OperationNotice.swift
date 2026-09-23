import Foundation

struct OperationNotice: Identifiable {
    let id = UUID()
    let title: String
    let message: String

    static func switchSucceeded(email: String, codexRunning: Bool) -> Self {
        let message = if codexRunning {
            "已切换到 \(email)。检测到 Codex 正在运行，请重启 Codex 使新账号生效。"
        } else {
            "已切换到 \(email)，新的登录状态已写入 Codex 配置。"
        }
        return Self(title: "切换成功", message: message)
    }

    static func error(_ message: String) -> Self {
        Self(title: "操作失败", message: message)
    }

    static func loginSucceeded(_ message: String) -> Self {
        Self(title: "登录成功", message: message)
    }

    static func deleteSucceeded(email: String) -> Self {
        Self(title: "删除成功", message: "已删除账号 \(email)。")
    }
}
