// CARGO center tab — payload hold with kind filters and priority rows.
// Exports: CargoView.

import SwiftUI

struct CargoView: View {
    @Environment(\.theme) private var theme
    let sectors: [Sector]
    @Binding var selectedMissionID: MissionID?
    @State private var filter: PayloadFilter = .all

    private var payloads: [Payload] {
        PayloadDeriver.payloads(from: sectors)
    }

    private var filtered: [Payload] {
        switch filter {
        case .all: return payloads
        case .kind(let kind): return payloads.filter { $0.kind == kind }
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: theme.spacing.md) {
                header
                filterChips
                ForEach(filtered) { payload in
                    payloadRow(payload)
                }
            }
            .padding(theme.spacing.md)
        }
    }

    private var header: some View {
        let critical = PayloadDeriver.countCritical(in: payloads)
        return HStack {
            MonoLabel(text: "cargo hold")
            Spacer()
            MonoLabel(text: "\(payloads.count) stowed · \(critical) critical", color: theme.ink2)
        }
    }

    private var filterChips: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: theme.spacing.xs) {
                chip(.all, count: payloads.count)
                ForEach(PayloadKind.allCases, id: \.self) { kind in
                    chip(.kind(kind), count: payloads.filter { $0.kind == kind }.count, label: kind.rawValue)
                }
            }
        }
    }

    private func chip(_ value: PayloadFilter, count: Int, label: String? = nil) -> some View {
        let title = label ?? "ALL"
        let selected = filter == value
        return ThemedButton(title: "\(title) \(count)", selected: selected) {
            filter = value
        }
    }

    private func payloadRow(_ payload: Payload) -> some View {
        let selected = selectedMissionID == payload.id
        return Button {
            selectedMissionID = payload.id
        } label: {
            HStack(spacing: theme.spacing.md) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(payload.name)
                        .font(theme.font(.body))
                        .foregroundStyle(theme.ink)
                    Text(payload.id)
                        .font(theme.font(.caption))
                        .foregroundStyle(theme.ink3)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                MonoLabel(text: payload.kind.rawValue, color: theme.accent)
                RarityChevrons(rarity: payload.rarity)
                    .frame(width: 160, alignment: .leading)
                MonoLabel(text: payload.sectorTag, color: theme.ink2)
                Text(payload.manifest)
                    .font(theme.font(.caption))
                    .foregroundStyle(theme.ink3)
                    .frame(width: 160, alignment: .trailing)
            }
            .padding(theme.spacing.sm)
            .background(selected ? theme.accent.opacity(0.08) : theme.bg)
            .overlay(alignment: .leading) {
                if selected { Rectangle().fill(theme.accent).frame(width: 2) }
            }
        }
        .buttonStyle(.plain)
    }
}

private enum PayloadFilter: Equatable {
    case all
    case kind(PayloadKind)
}
