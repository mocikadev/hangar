import SwiftUI

struct QuotaContent: View {
    let card: AccountCardRecord

    var body: some View {
        if card.account.stale || card.quotaState == .stale {
            Label("需要重新登录", systemImage: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange)
        } else if let remaining = card.quota?.weeklyRemaining {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text("周剩余")
                        .foregroundStyle(.secondary)
                    Spacer()
                    Text(remaining, format: .number)
                        .bold()
                    Text("%")
                        .foregroundStyle(.secondary)
                }
                ProgressView("周剩余额度", value: Double(remaining), total: 100)
                    .labelsHidden()
                    .accessibilityHidden(true)
                Text(weeklyResetDescription())
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(resetCountDescription())
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(resetExpiryDescription())
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if let fetchedAt = card.quotaFetchedAt {
                    Text(cacheDescription(fetchedAt: fetchedAt))
                        .font(.caption)
                        .foregroundStyle(card.quotaIsFresh ? Color.secondary : Color.orange)
                }
            }
        } else {
            switch card.quotaState {
            case .pending, .loading:
                HStack {
                    ProgressView()
                        .controlSize(.small)
                        .accessibilityHidden(true)
                    Text("正在获取周额度…")
                        .foregroundStyle(.secondary)
                }
                .accessibilityElement(children: .combine)
            case .failed:
                Label("额度获取失败", systemImage: "wifi.exclamationmark")
                    .foregroundStyle(.red)
            case .unknown, .success:
                Label("周额度未知", systemImage: "questionmark.circle")
                    .foregroundStyle(.secondary)
            case .stale:
                EmptyView()
            }
        }
    }

    private func cacheDescription(fetchedAt: Int64) -> String {
        let time = formattedTime(fetchedAt)
        if card.quotaState == .failed {
            return "刷新失败 · 上次成功 \(time)"
        }
        if !card.quotaIsFresh {
            return "旧数据 · 更新于 \(time)"
        }
        if card.quotaState == .loading {
            return "更新于 \(time) · 正在刷新"
        }
        return "更新于 \(time)"
    }

    private func formattedTime(_ timestamp: Int64) -> String {
        Date(timeIntervalSince1970: TimeInterval(timestamp))
            .formatted(date: .abbreviated, time: .shortened)
    }

    private func weeklyResetDescription() -> String {
        guard let resetAt = card.quota?.weeklyResetAt else {
            return "周额度重置时间未知"
        }
        return "周额度重置于 \(formattedTime(resetAt))"
    }

    private func resetCountDescription() -> String {
        guard let available = card.quota?.resetAvailable else {
            return "重置卡数量未知"
        }
        return "重置卡 \(available) 张"
    }

    private func resetExpiryDescription() -> String {
        guard let available = card.quota?.resetAvailable else {
            return "重置卡到期时间未知"
        }
        guard available > 0 else {
            return "暂无可用重置卡"
        }
        guard let expiry = card.quota?.resetNextExpiry else {
            return "最早到期时间未知"
        }
        return "最早到期 \(formattedTime(expiry))"
    }
}
