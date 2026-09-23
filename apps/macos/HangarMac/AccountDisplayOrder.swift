/// macOS 展示顺序；不改变账号库顺序或 Core 的推荐结果。
enum AccountDisplayOrder {
    static func sorted(_ cards: [AccountCardRecord]) -> [AccountCardRecord] {
        cards.enumerated().sorted { left, right in
            let leftPriority = priority(left.element)
            let rightPriority = priority(right.element)
            if leftPriority != rightPriority {
                return leftPriority < rightPriority
            }
            if leftPriority == 2 {
                let leftWeekly = left.element.quota?.weeklyRemaining ?? 0
                let rightWeekly = right.element.quota?.weeklyRemaining ?? 0
                if leftWeekly != rightWeekly {
                    return leftWeekly > rightWeekly
                }
            }
            return left.offset < right.offset
        }.map(\.element)
    }

    static func preservingPositions(
        _ cards: [AccountCardRecord],
        previous: [AccountCardRecord]
    ) -> [AccountCardRecord] {
        let positions = Dictionary(
            uniqueKeysWithValues: previous.enumerated().map { ($0.element.account.id, $0.offset) }
        )
        return cards.enumerated().sorted { left, right in
            let leftPosition = positions[left.element.account.id] ?? previous.count + left.offset
            let rightPosition = positions[right.element.account.id] ?? previous.count + right.offset
            return leftPosition < rightPosition
        }.map(\.element)
    }

    private static func priority(_ card: AccountCardRecord) -> Int {
        if card.account.current { return 0 }
        if card.account.stale { return 4 }
        if card.recommended { return 1 }
        if card.quota?.weeklyRemaining != nil { return 2 }
        return 3
    }
}
