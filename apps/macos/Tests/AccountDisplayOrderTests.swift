// Standalone projection test; compile with HangarMac/AccountDisplayOrder.swift.
// These minimal records only model display fields and contain no credentials.
struct AccountRecord {
    let id: String
    let current: Bool
    let stale: Bool
}

struct QuotaRecord {
    let weeklyRemaining: Int?
}

struct AccountCardRecord {
    let account: AccountRecord
    let quota: QuotaRecord?
    let recommended: Bool
}

@main
enum AccountDisplayOrderTests {
    static func main() {
        let cards = [
            card("unknown"),
            card("equal-first", weekly: 20),
            card("current", current: true, weekly: 2),
            card("recommended", recommended: true, weekly: 15),
            card("highest", weekly: 80),
            card("stale", stale: true, weekly: 90),
            card("equal-second", weekly: 20)
        ]
        let ordered = AccountDisplayOrder.sorted(cards)
        expectIDs(
            ordered,
            ["current", "recommended", "highest", "equal-first", "equal-second", "unknown", "stale"]
        )

        let partialRefresh = [
            card("highest", weekly: 5),
            card("new", weekly: 99),
            card("unknown", weekly: 70),
            card("recommended", recommended: true, weekly: 15),
            card("equal-second", weekly: 20),
            card("current", current: true, weekly: 2),
            card("equal-first", weekly: 20)
        ]
        expectIDs(
            AccountDisplayOrder.preservingPositions(partialRefresh, previous: ordered),
            ["current", "recommended", "highest", "equal-first", "equal-second", "unknown", "new"]
        )
        expectIDs(
            AccountDisplayOrder.sorted(partialRefresh),
            ["current", "recommended", "new", "unknown", "equal-second", "equal-first", "highest"]
        )
    }

    private static func card(
        _ id: String,
        current: Bool = false,
        stale: Bool = false,
        recommended: Bool = false,
        weekly: Int? = nil
    ) -> AccountCardRecord {
        AccountCardRecord(
            account: AccountRecord(id: id, current: current, stale: stale),
            quota: weekly.map { QuotaRecord(weeklyRemaining: $0) },
            recommended: recommended
        )
    }

    private static func expectIDs(_ cards: [AccountCardRecord], _ expected: [String]) {
        let actual = cards.map(\.account.id)
        precondition(actual == expected, "Unexpected display order: \(actual)")
    }
}
