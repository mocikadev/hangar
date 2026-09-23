import SwiftUI

struct AccountCardView: View {
    @Environment(\.colorSchemeContrast) private var colorSchemeContrast
    let card: AccountCardRecord
    let isSwitching: Bool
    let isDeleting: Bool
    let actionsDisabled: Bool
    let switchAction: () -> Void
    let reauthAction: () -> Void
    let deleteAction: () -> Void

    @State private var confirmsDeletion = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(card.account.email)
                        .font(.headline)
                        .lineLimit(1)
                        .help(card.account.email)
                    Text(card.quota?.plan ?? "套餐未知")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if card.recommended {
                    Label("推荐", systemImage: "sparkles")
                        .foregroundStyle(.blue)
                }
            }

            QuotaContent(card: card)
            Spacer(minLength: 0)

            HStack {
                if card.account.stale {
                    Button("重新登录", action: reauthAction)
                        .buttonStyle(.borderedProminent)
                        .disabled(actionsDisabled)
                        .accessibilityLabel("重新登录账号 \(card.account.email)")
                } else if card.account.current {
                    Label("使用中", systemImage: "checkmark.circle.fill")
                        .foregroundStyle(.green)
                } else {
                    Button("切换到此账号", action: switchAction)
                        .buttonStyle(.borderedProminent)
                        .disabled(actionsDisabled)
                        .accessibilityLabel("切换到账号 \(card.account.email)")
                }

                Spacer()

                if !card.account.stale || !card.account.current {
                    Menu {
                        if !card.account.stale {
                            Button("重新登录", systemImage: "arrow.clockwise.circle", action: reauthAction)
                        }
                        if !card.account.current {
                            Divider()
                            Button("删除账号", systemImage: "trash", role: .destructive) {
                                confirmsDeletion = true
                            }
                        }
                    } label: {
                        Label("更多", systemImage: "ellipsis.circle")
                            .accessibilityLabel("更多账号操作，\(card.account.email)")
                    }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                    .disabled(actionsDisabled)
                }

                if isSwitching || isDeleting {
                    ProgressView()
                        .controlSize(.small)
                        .accessibilityLabel(isDeleting ? "正在删除账号" : "正在切换账号")
                }
            }
        }
        .frame(maxWidth: .infinity, minHeight: 220, alignment: .topLeading)
        .padding()
        .background(.regularMaterial, in: .rect(cornerRadius: 14))
        .overlay {
            RoundedRectangle(cornerRadius: 14)
                .stroke(
                    card.account.current ? Color.accentColor : inactiveStrokeColor,
                    lineWidth: colorSchemeContrast == .increased ? 2 : 1
                )
        }
        .confirmationDialog(
            "删除账号？",
            isPresented: $confirmsDeletion,
            titleVisibility: .visible
        ) {
            Button("删除", role: .destructive, action: deleteAction)
            Button("取消", role: .cancel) {}
        } message: {
            Text("将从 Hangar 账号库删除 \(card.account.email)。当前账号不能删除。")
        }
        .accessibilityElement(children: .contain)
    }

    private var inactiveStrokeColor: Color {
        colorSchemeContrast == .increased ? .secondary : .secondary.opacity(0.2)
    }

}
