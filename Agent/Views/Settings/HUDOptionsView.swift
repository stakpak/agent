import SwiftUI

/// HUD (Heads-Up Display) options popover
struct HUDOptionsView: View {
    @Bindable var viewModel: AgentViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("HUD")
                .font(.headline)
            Text("Heads-Up Display for LLM Output. Press ⌘B to show/hide during a task.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            Divider()

            VStack(alignment: .leading, spacing: 4) {
                Text("Terminal Speed")
                    .font(.caption)
                    .foregroundStyle(.green)
                Picker("", selection: $viewModel.terminalSpeed) {
                    ForEach(AgentViewModel.TerminalSpeed.allCases, id: \.self) { speed in
                        Text(speed.label).tag(speed)
                    }
                }
                .pickerStyle(.segmented)
                .tint(.green)
            }

            HStack {
                Text("Scan Lines").font(.caption)
                Spacer()
                Toggle("", isOn: $viewModel.scanLinesEnabled)
                    .toggleStyle(.switch)
                    .controlSize(.mini)
                    .tint(.green)
            }
        }
        .padding(16)
        .frame(width: 360)
    }
}
