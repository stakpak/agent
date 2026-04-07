import AppKit

/// NSMenu delegate that builds the 🦾 Agents menu dynamically from RecentAgentsService.
@MainActor
final class AgentsMenuDelegate: NSObject, NSMenuDelegate {
    static let shared = AgentsMenuDelegate()
    weak var viewModel: AgentViewModel?

    func menuNeedsUpdate(_ menu: NSMenu) {
        buildMenu(menu)
    }

    private func buildMenu(_ menu: NSMenu) {
        menu.removeAllItems()
        let entries = RecentAgentsService.shared.entries.filter { !$0.agentName.trimmingCharacters(in: .whitespaces).isEmpty }

        if entries.isEmpty {
            let item = NSMenuItem(title: "No recent agents", action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
            return
        }

        // Group by agent name
        var seen: [String: [RecentAgentsService.AgentEntry]] = [:]
        var order: [String] = []
        for entry in entries {
            if seen[entry.agentName] == nil {
                order.append(entry.agentName)
                seen[entry.agentName] = []
            }
            seen[entry.agentName]?.append(entry)
        }

        // Sort: green (success) first, orange (cancelled) second, red (failed) last, pending at end
        let statusOrder: [RecentAgentsService.RunStatus] = [.success, .cancelled, .failed, .pending]
        let sorted = order.sorted { a, b in
            let sa = seen[a]?.first?.status ?? .pending
            let sb = seen[b]?.first?.status ?? .pending
            let ia = statusOrder.firstIndex(of: sa) ?? 3
            let ib = statusOrder.firstIndex(of: sb) ?? 3
            return ia < ib
        }

        for name in sorted {
            guard let group = seen[name] else { continue }

            let agentSubmenu = NSMenu(title: name)
            // Color the agent name by most recent run status
            let statusColor: NSColor = {
                guard let latest = group.first else { return .labelColor }
                switch latest.status {
                case .success:   return .systemGreen
                case .cancelled: return .systemOrange
                case .failed:    return .systemRed
                case .pending:   return .labelColor
                }
            }()
            let agentItem = NSMenuItem(title: "", action: nil, keyEquivalent: "")
            agentItem.attributedTitle = NSAttributedString(
                string: name,
                attributes: [.foregroundColor: statusColor, .font: NSFont.menuFont(ofSize: 0)]
            )
            agentItem.submenu = agentSubmenu

            // Run submenu with play SF Symbol
            let runMenu = NSMenu(title: "Run")
            let runItem = NSMenuItem(title: "Run", action: nil, keyEquivalent: "")
            runItem.image = NSImage(systemSymbolName: "play.fill", accessibilityDescription: "Run")
            runItem.submenu = runMenu
            for entry in group {
                let label = entry.arguments.isEmpty ? entry.agentName : entry.arguments
                let item = NSMenuItem(title: label, action: #selector(playAgent(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = entry.populatedPrompt
                runMenu.addItem(item)
            }
            agentSubmenu.addItem(runItem)

            // Edit submenu with pencil SF Symbol
            let editMenu = NSMenu(title: "Edit")
            let editItem = NSMenuItem(title: "Edit", action: nil, keyEquivalent: "")
            editItem.image = NSImage(systemSymbolName: "pencil", accessibilityDescription: "Edit")
            editItem.submenu = editMenu
            for entry in group {
                let label = entry.arguments.isEmpty ? entry.agentName : entry.arguments
                let item = NSMenuItem(title: label, action: #selector(editAgent(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = entry.populatedPrompt
                editMenu.addItem(item)
            }
            agentSubmenu.addItem(editItem)

            // Remove submenu with trash SF Symbol
            let removeMenu = NSMenu(title: "Remove")
            let removeItem = NSMenuItem(title: "Remove", action: nil, keyEquivalent: "")
            removeItem.image = NSImage(systemSymbolName: "trash", accessibilityDescription: "Remove")
            removeItem.submenu = removeMenu
            for entry in group {
                let label = entry.arguments.isEmpty ? entry.agentName : entry.arguments
                let item = NSMenuItem(title: label, action: #selector(removeAgent(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = entry.id
                removeMenu.addItem(item)
            }
            if group.count > 1 {
                removeMenu.addItem(.separator())
                let allItem = NSMenuItem(title: "Remove All", action: #selector(removeAgentGroup(_:)), keyEquivalent: "")
                allItem.target = self
                allItem.representedObject = name
                removeMenu.addItem(allItem)
            }
            agentSubmenu.addItem(removeItem)

            menu.addItem(agentItem)
        }

        menu.addItem(.separator())

        let clearItem = NSMenuItem(title: "Clear Recent Agents", action: #selector(clearAgents), keyEquivalent: "")
        clearItem.target = self
        menu.addItem(clearItem)
    }

    private func addAgentItems(to menu: NSMenu, entry: RecentAgentsService.AgentEntry) {
        let playItem = NSMenuItem(title: "▶ \(entry.menuLabel)", action: #selector(playAgent(_:)), keyEquivalent: "")
        playItem.target = self
        playItem.representedObject = entry.populatedPrompt
        menu.addItem(playItem)

        let editItem = NSMenuItem(title: "✏️ \(entry.menuLabel)", action: #selector(editAgent(_:)), keyEquivalent: "")
        editItem.target = self
        editItem.representedObject = entry.populatedPrompt
        menu.addItem(editItem)
    }

    @objc private func playAgent(_ sender: NSMenuItem) {
        guard let prompt = sender.representedObject as? String,
              let vm = viewModel else { return }
        let parts = prompt.components(separatedBy: " ")
        let name = parts.count > 1 ? parts[1] : prompt
        let args = parts.count > 2 ? parts.dropFirst(2).joined(separator: " ") : ""
        // Defer to next run loop so the menu closes immediately — no spinner
        DispatchQueue.main.async {
            Task { await vm.runAgentDirect(name: name, arguments: args) }
        }
    }

    @objc private func editAgent(_ sender: NSMenuItem) {
        guard let prompt = sender.representedObject as? String,
              let vm = viewModel else { return }
        if let selId = vm.selectedTabId,
           let tab = vm.tab(for: selId) {
            tab.taskInput = prompt
        } else {
            vm.taskInput = prompt
        }
    }

    @objc private func removeAgent(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? UUID else { return }
        RecentAgentsService.shared.removeById(id)
    }

    @objc private func removeAgentGroup(_ sender: NSMenuItem) {
        guard let name = sender.representedObject as? String else { return }
        RecentAgentsService.shared.removeAgent(name: name)
    }

    @objc private func clearAgents() {
        RecentAgentsService.shared.clearAll()
    }
}
