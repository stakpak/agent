import SwiftUI

struct MessagesView: View {
    @Bindable var viewModel: AgentViewModel
    @State private var renderKey = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header
            Text("Messages Monitor")
                .font(.headline)
            
            Text("Monitor iMessage for \"Agent!\" commands.")
                .font(.caption)
                .foregroundStyle(.secondary)
            
            HStack {
                Picker("Active", selection: $viewModel.messageFilter) {
                    ForEach(AgentViewModel.MessageFilter.allCases, id: \.self) { filter in
                        Text(filter.rawValue).tag(filter)
                    }
                }
                .pickerStyle(.segmented)
                
                Spacer()
                
                Toggle("", isOn: $viewModel.messagesMonitorEnabled)
                    .toggleStyle(.switch)
                    .controlSize(.mini)
                    .tint(.blue)
                    .labelsHidden()
            }

            Divider()

            if viewModel.filteredRecipients.isEmpty {
                VStack(spacing: 12) {
                    Image(systemName: "message")
                        .font(.system(size: 32))
                        .foregroundStyle(.secondary)
                    Text("No recipients yet")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Text("Recipients appear here as messages arrive.")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 20)
            } else {
                HStack {
                    Text("Check recipients to act on \"Agent!\" commands:")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("All") {
                        let filtered = Set(viewModel.filteredRecipients.map(\.id))
                        viewModel.enabledHandleIds.formUnion(filtered)
                    }
                    .buttonStyle(.bordered).controlSize(.mini)
                    Button("None") {
                        let filtered = Set(viewModel.filteredRecipients.map(\.id))
                        viewModel.enabledHandleIds.subtract(filtered)
                    }
                    .buttonStyle(.bordered).controlSize(.mini)
                    Button("Clear") {
                        viewModel.messageRecipients.removeAll()
                        viewModel.enabledHandleIds.removeAll()
                        UserDefaults.standard.removeObject(forKey: "agentDiscoveredHandles")
                        UserDefaults.standard.removeObject(forKey: "agentDiscoveredServices")
                        UserDefaults.standard.removeObject(forKey: "agentDiscoveredFromMe")
                    }
                    .buttonStyle(.bordered).controlSize(.mini)
                }

                ScrollView {
                    LazyVStack(spacing: 4) {
                        ForEach(viewModel.filteredRecipients) { recipient in
                            recipientRow(recipient)
                        }
                    }
                }
                .id(renderKey)
            }

            Divider()

            VStack(alignment: .leading, spacing: 4) {
                Text("Send \"Agent! <prompt>\" from a checked recipient to trigger a task.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text("Unchecked recipients are logged but not acted on. Each recipient must be approved.")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }

            if !AgentViewModel.checkFullDiskAccess() {
                Divider()
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.yellow)
                    Text("Full Disk Access required to read Messages.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("Open Settings") {
                        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")!)
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
            }
        }
        .padding(16)
        .padding(.bottom, 15)
        .frame(width: 380)
        .frame(maxHeight: 480)
        .onAppear {
            viewModel.refreshMessageRecipients()
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
                renderKey.toggle()
            }
        }
    }

    @ViewBuilder
    private func recipientRow(_ recipient: AgentViewModel.MessageRecipient) -> some View {
        let isEnabled = viewModel.enabledHandleIds.contains(recipient.id)

        HStack(spacing: 10) {
            Toggle("", isOn: Binding(
                get: { viewModel.enabledHandleIds.contains(recipient.id) },
                set: { newValue in
                    if newValue {
                        viewModel.enabledHandleIds.insert(recipient.id)
                    } else {
                        viewModel.enabledHandleIds.remove(recipient.id)
                    }
                }
            ))
            .toggleStyle(.switch)
            .controlSize(.mini)
            .tint(.blue)

            Text(recipient.id)
                .font(.subheadline)
                .foregroundStyle(isEnabled ? .primary : .secondary)
                .lineLimit(1)

            Spacer()

            Text(recipient.service)
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 4)
        .padding(.horizontal, 8)
        .background(isEnabled ? Color.blue.opacity(0.05) : .clear)
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }
}
