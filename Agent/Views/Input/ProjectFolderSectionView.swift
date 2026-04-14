import SwiftUI

/// Project folder section with token badge - displayed below header
struct ProjectFolderSectionView: View {
    @Bindable var viewModel: AgentViewModel
    var selectedTab: ScriptTab?

    var body: some View {
        if let tab = selectedTab {
            // Per-tab project folder (when a tab is selected)
            HStack(spacing: 4) {
                ProjectFolderField(
                    projectFolder: Binding(
                        get: { tab.projectFolder.isEmpty ? viewModel.projectFolder : tab.projectFolder },
                        set: { tab.projectFolder = $0; viewModel.persistScriptTabs() }
                    )
                )
                TokenBadge(
                    taskIn: viewModel.taskInputTokens,
                    taskOut: viewModel.taskOutputTokens,
                    sessionIn: viewModel.sessionInputTokens,
                    sessionOut: viewModel.sessionOutputTokens,
                    providerName: viewModel.selectedProvider.displayName,
                    modelName: viewModel.globalModelForProvider(viewModel.selectedProvider),
                    budgetUsedFraction: viewModel.budgetUsedFraction
                )
            }
            .id(tab.id)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
        } else {
            // Project folder/file (main tab)
            HStack(spacing: 4) {
                ProjectFolderField(projectFolder: $viewModel.projectFolder)
                TokenBadge(
                    taskIn: viewModel.taskInputTokens,
                    taskOut: viewModel.taskOutputTokens,
                    sessionIn: viewModel.sessionInputTokens,
                    sessionOut: viewModel.sessionOutputTokens,
                    providerName: viewModel.selectedProvider.displayName,
                    modelName: viewModel.globalModelForProvider(viewModel.selectedProvider),
                    budgetUsedFraction: viewModel.budgetUsedFraction
                )
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
        }
    }
}
