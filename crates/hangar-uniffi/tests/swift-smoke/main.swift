import Foundation

let info = bridgeInfo()
guard info.interfaceVersion == 1 else {
    fatalError("unexpected bridge interface version: \(info.interfaceVersion)")
}
guard !info.coreVersion.isEmpty else {
    fatalError("empty core version")
}
print("hangar-uniffi \(info.interfaceVersion) \(info.coreVersion)")
