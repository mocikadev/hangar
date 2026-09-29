@main
enum RefreshPresentationTests {
    static func main() {
        precondition(
            RefreshPresentation.message(accountCount: 6) == "正在刷新 6 个账号的周额度…",
            "Unexpected refresh message"
        )
    }
}
