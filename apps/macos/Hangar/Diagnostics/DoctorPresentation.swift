import Foundation

struct DoctorPresentation: Identifiable {
    let id = UUID()
    let lines: [String]
    let issueCount: UInt64
}
