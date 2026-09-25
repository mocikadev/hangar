import Foundation

@main
enum DateFormattingTests {
    static func main() {
        let timeZone = TimeZone(secondsFromGMT: 8 * 60 * 60)!
        let actual = HangarDateFormatting.string(
            from: 1_792_740_240,
            timeZone: timeZone
        )
        precondition(actual == "2026年10月23日 15:24", "Unexpected date format: \(actual)")
    }
}
