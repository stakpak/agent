import SwiftUI
import SwiftData

/// App identifiers — single source of truth for bundle ID, XPC services, plists, etc.
enum AppConstants {
    static let bundleID = "Agent.app.toddbruss"
    static let subsystem = bundleID
    static let helperID = "\(bundleID).helper"
    static let userID = "\(bundleID).user"
    static let helperPlist = "\(bundleID).helper.plist"
    static let userPlist = "\(bundleID).user.plist"

    /// Preferred shell path for in-app Process() calls.
    /// Reads from UserDefaults "agentShellPath", defaults to /bin/zsh.
    static var shellPath: String {
        UserDefaults.standard.string(forKey: "agentShellPath") ?? "/bin/zsh"
    }
}

extension Notification.Name {
    static let appWillQuit = Notification.Name("appWillQuit")
    /// Posted when a tab's or main activityLog changes. object = tab UUID (or nil for main)
    static let activityLogDidChange = Notification.Name("activityLogDidChange")

    // AskUserQuestion — mid-task dialog
    static let askUserQuestion = Notification.Name("askUserQuestion")
    static let userQuestionAnswered = Notification.Name("userQuestionAnswered")

    // Menu command notifications — posted by Shortcuts menu, handled by ContentView
    static let menuToggleChevrons = Notification.Name("menuToggleChevrons")
    static let menuToggleOverlay = Notification.Name("menuToggleOverlay")
    static let menuRunTask = Notification.Name("menuRunTask")
    static let menuCancelTask = Notification.Name("menuCancelTask")
    static let menuFind = Notification.Name("menuFind")
    static let menuNewTab = Notification.Name("menuNewTab")
    static let menuCloseTab = Notification.Name("menuCloseTab")
    static let menuNextTab = Notification.Name("menuNextTab")
    static let menuPrevTab = Notification.Name("menuPrevTab")
    static let menuClearAll = Notification.Name("menuClearAll")
    static let menuClearLog = Notification.Name("menuClearLog")
    static let menuClearLLM = Notification.Name("menuClearLLM")
    static let menuClearHistory = Notification.Name("menuClearHistory")
    static let menuClearTasks = Notification.Name("menuClearTasks")
    static let menuClearTokens = Notification.Name("menuClearTokens")
}

private func post(_ name: Notification.Name) {
    NotificationCenter.default.post(name: name, object: nil)
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // Initialize accessibility enabled defaults on startup
        // This ensures the UserDefaults keys exist before any isRestricted() checks
        _ = AccessibilityEnabled.shared

        // Persist window frame across launches and tiling
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            if let window = NSApplication.shared.windows.first {
                window.setFrameAutosaveName("AgentMainWindow")
            }
        }

        // Insert 🦾 Agents menu immediately + every time the app becomes active.
        // Why both: SwiftUI's Commands modifier rebuilds the main menu AFTER
        // applicationDidFinishLaunching, so a single insert here can be stomped
        // on. didBecomeActive fires after SwiftUI's menu setup completes AND
        // every time the user switches back to the app — guaranteed reinsert.
        insertAgentsMenu()
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(reinsertAgentsMenuIfMissing),
            name: NSApplication.didBecomeActiveNotification,
            object: nil
        )
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(reinsertAgentsMenuIfMissing),
            name: NSApplication.didUpdateNotification,
            object: nil
        )
    }

    @MainActor @objc private func reinsertAgentsMenuIfMissing() {
        guard let menu = NSApplication.shared.mainMenu,
              !menu.items.contains(where: { $0.title.contains("Agents") }) else { return }
        insertAgentsMenu()
    }

    @MainActor private func insertAgentsMenu() {
        guard let mainMenu = NSApplication.shared.mainMenu else { return }
        // Remove any existing Agents menu (SwiftUI-added or stale)
        while let idx = mainMenu.items.firstIndex(where: { $0.title.contains("Agents") }) {
            mainMenu.removeItem(at: idx)
        }
        // Create NSMenu version and insert at position 1 (after app menu, before File)
        let agentsMenu = NSMenu(title: "🦾 Agents")
        let agentsItem = NSMenuItem(title: "🦾 Agents", action: nil, keyEquivalent: "")
        agentsItem.submenu = agentsMenu
        agentsMenu.delegate = AgentsMenuDelegate.shared
        let insertIdx = min(1, mainMenu.items.count)
        mainMenu.insertItem(agentsItem, at: insertIdx)
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        // Tell the view model to stop all running tasks, MCP servers, etc.
        NotificationCenter.default.post(name: .appWillQuit, object: nil)
        // Drain the script compilation queue before exit to prevent a deadlock
        // where C++ static destructors (DoIOSInit) try to fflush(stdout) while
        // the compilation queue holds the flockfile(stdout) lock.
        ScriptService.drainCompilationQueue()
        return .terminateNow
    }
}

@main
struct AgentApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) var appDelegate
    var body: some Scene {
        WindowGroup {
            ContentView()
                .task {
                    // Initialize SwiftData chat history store
                    ChatHistoryStore.shared.migrateFromUserDefaults()

                    await MCPService.shared.startAutoStartServers()
                    // Sync registry enabled flags with actual connection state
                    let registry = MCPServerRegistry.shared
                    for server in registry.servers {
                        let connected = MCPService.shared.connectedServerIds.contains(server.id)
                        if server.enabled != connected {
                            registry.setEnabled(server.id, connected)
                        }
                    }
                }
        }
        .windowResizability(.contentSize)
        .windowToolbarStyle(.unified(showsTitle: false))
        .commands {
            // Remove system Cmd+N so our shortcuts aren't hidden
            CommandGroup(replacing: .newItem) {}
            CommandGroup(after: .windowArrangement) {
                Button("System Prompts") {
                    SystemPromptWindow.shared.show()
                }
                .keyboardShortcut("p", modifiers: [.command, .shift])
            }
            CommandMenu("Shortcuts") {
                Button("Toggle LLM Chevrons") { post(.menuToggleChevrons) }
                    .keyboardShortcut("d", modifiers: .command)
                Button("Toggle LLM Overlay") { post(.menuToggleOverlay) }
                    .keyboardShortcut("b", modifiers: .command)
                Divider()
                Button("Run Task") { post(.menuRunTask) }
                    .keyboardShortcut(.return, modifiers: .command)
                Button("Cancel Task") { post(.menuCancelTask) }
                    .keyboardShortcut(.escape, modifiers: [])
                Divider()
                Button("Find") { post(.menuFind) }
                    .keyboardShortcut("f", modifiers: .command)
                Button("New Tab") { post(.menuNewTab) }
                    .keyboardShortcut("t", modifiers: .command)
                Button("Close Tab") { post(.menuCloseTab) }
                    .keyboardShortcut("w", modifiers: .command)
                Divider()
                Button("Next Tab") { post(.menuNextTab) }
                    .keyboardShortcut("]", modifiers: .command)
                Button("Previous Tab") { post(.menuPrevTab) }
                    .keyboardShortcut("[", modifiers: .command)
                Divider()
                Button("Clear All (log, LLM, history, tasks, tokens)") { post(.menuClearAll) }
                    .keyboardShortcut("k", modifiers: [.command, .shift])
                Button("Clear Log") { post(.menuClearLog) }
                    .keyboardShortcut("l", modifiers: .command)
                Button("Clear LLM Output") { post(.menuClearLLM) }
                    .keyboardShortcut("l", modifiers: [.command, .shift])
                Button("Clear History") { post(.menuClearHistory) }
                    .keyboardShortcut("h", modifiers: [.command, .shift])
                Button("Clear Tasks") { post(.menuClearTasks) }
                    .keyboardShortcut("j", modifiers: [.command, .shift])
                Button("Clear Tokens") { post(.menuClearTokens) }
                    .keyboardShortcut("u", modifiers: [.command, .shift])
            }
        }
    }
}
