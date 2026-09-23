import SwiftUI

struct DoctorSheetView: View {
    @Environment(\.dismiss) private var dismiss
    let report: DoctorPresentation

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Label(
                report.issueCount == 0 ? "自检通过" : "发现 \(report.issueCount) 个问题",
                systemImage: report.issueCount == 0 ? "checkmark.circle.fill" : "exclamationmark.triangle.fill"
            )
            .font(.title2.bold())
            .foregroundStyle(report.issueCount == 0 ? .green : .orange)

            ScrollView {
                Text(report.lines.joined(separator: "\n"))
                    .font(.system(.body, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
            }
            .padding(12)
            .background(.quaternary, in: .rect(cornerRadius: 10))

            HStack {
                Spacer()
                Button("关闭") {
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(24)
        .frame(minWidth: 480, idealWidth: 620, minHeight: 360, idealHeight: 420)
    }
}
