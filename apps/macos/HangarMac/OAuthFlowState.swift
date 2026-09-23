import Foundation

struct OAuthFlowState: Identifiable {
    let id: UInt64
    let authorizationURL: URL
    let title: String
    let successMessage: String
    var phase: LoginPhase = .waiting
    var message: String?

    var acceptsManualCallback: Bool {
        phase == .waiting
    }
}
