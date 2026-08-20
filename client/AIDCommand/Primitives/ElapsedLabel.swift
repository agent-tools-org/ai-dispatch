// Live elapsed readout — ticks from startedAt while a mission is RUN.
// Exports: ElapsedLabel.

import SwiftUI

struct ElapsedLabel: View {
    let mission: Mission

    var body: some View {
        if mission.state == .run, let startedAt = mission.startedAt {
            TimelineView(.periodic(from: .now, by: 1)) { context in
                Text(FleetFormatters.elapsed(seconds: seconds(since: startedAt, at: context.date)))
            }
        } else {
            Text(FleetFormatters.elapsed(seconds: mission.elapsedSeconds))
        }
    }

    private func seconds(since startedAt: Date, at now: Date) -> Int {
        max(0, Int(now.timeIntervalSince(startedAt)))
    }
}
