//
//  ServicesPopover.swift
//  Agent
//
//  Extracted from ContentView.swift
//

import SwiftUI

struct ServicesPopover: View {
    @Bindable var viewModel: AgentViewModel
    
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Services")
                .font(.headline)
            
            Text("Background agents for shell commands and automation.")
                .font(.caption)
                .foregroundStyle(.secondary)
                        
            Grid(alignment: .leading, verticalSpacing: 10) {
                GridRow {
                    StatusDot(
                        isActive: viewModel.userServiceActive,
                        wasActive: viewModel.userWasActive,
                        isBusy: viewModel.isRunning,
                        enabled: viewModel.userEnabled
                    )
                    Text("User Agent")
                        .font(.caption)
                    Toggle("", isOn: $viewModel.userEnabled)
                        .toggleStyle(.switch)
                        .controlSize(.mini)
                        .tint(.green)
                        .labelsHidden()
                }
                GridRow {
                    StatusDot(
                        isActive: viewModel.rootServiceActive,
                        wasActive: viewModel.rootWasActive,
                        isBusy: viewModel.isRunning,
                        enabled: viewModel.rootEnabled
                    )
                    Text("Daemon Agent")
                        .font(.caption)
                    Toggle("", isOn: $viewModel.rootEnabled)
                        .toggleStyle(.switch)
                        .controlSize(.mini)
                        .tint(.green)
                        .labelsHidden()
                }
            }
            
            Divider()
            
            // Action Buttons
            HStack(spacing: 8) {
                Button("Unregister") {
                    viewModel.unregisterAgent()
                    viewModel.unregisterDaemon()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                
                Button("Register") {
                    viewModel.registerAgent()
                    viewModel.registerDaemon()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                
                Button("Connect") {
                    viewModel.testConnection()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
        }
        .padding(16)
        .frame(width: 320)
    }
}
