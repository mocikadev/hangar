import SwiftUI

struct ContentView: View {
    let model: HangarModel

    var body: some View {
        DashboardView(model: model)
    }
}
