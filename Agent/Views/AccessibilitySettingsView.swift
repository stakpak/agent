import AgentAccess
import SwiftUI

/// Manages global accessibility on/off toggle.
@MainActor @Observable
final class AccessibilityEnabled {
    static let shared = AccessibilityEnabled()

    /// Global on/off for all accessibility automation
    var accessibilityGlobalEnabled: Bool {
        didSet { UserDefaults.standard.set(accessibilityGlobalEnabled, forKey: "AccessibilityGlobalEnabled") }
    }

    private init() {
        self.accessibilityGlobalEnabled = UserDefaults.standard.object(forKey: "AccessibilityGlobalEnabled") as? Bool ?? true
    }

    /// Returns true if accessibility is OFF (all actions blocked).
    func isRestricted(_ id: String) -> Bool {
        !accessibilityGlobalEnabled
    }

    func isEnabled(_ id: String) -> Bool {
        accessibilityGlobalEnabled
    }
}

// MARK: - View

struct AccessibilitySettingsView: View {
    @Bindable var settings = AccessibilityEnabled.shared

    @State private var hasAccessibility = AccessibilityService.hasAccessibilityPermission()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                // Accessibility Permission
                HStack(spacing: 8) {
                    Circle()
                        .fill(hasAccessibility ? Color.green : Color.red.opacity(0.6))
                        .frame(width: 8, height: 8)
                    Text("Accessibility: \(hasAccessibility ? "Granted" : "Not Granted")")
                        .font(.caption)
                        .foregroundStyle(hasAccessibility ? .green : .red)
                    Spacer()
                    if !hasAccessibility {
                        Button("Request Access") {
                            _ = AccessibilityService.requestAccessibilityPermission()
                            DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
                                hasAccessibility = AccessibilityService.hasAccessibilityPermission()
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.small)
                    }
                }

                Divider()

                Toggle(isOn: Binding(
                    get: { settings.accessibilityGlobalEnabled },
                    set: { settings.accessibilityGlobalEnabled = $0 }
                )) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Accessibility Automation")
                            .font(.headline)
                        Text(settings.accessibilityGlobalEnabled ? "Agent can interact with UI elements via AXorcist" : "All accessibility actions are blocked")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                .toggleStyle(.switch)
                .controlSize(.mini)

                Divider()

                // Apple Events Permission
                HStack(spacing: 8) {
                    Circle()
                        .fill(Color.green.opacity(0.6))
                        .frame(width: 8, height: 8)
                    Text("Apple Events: Granted on first use of each application")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("Settings") {
                        if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation") {
                            NSWorkspace.shared.open(url)
                        }
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
            }
            .padding(16)
            .padding(.bottom, 15)
        }
        .frame(width: 500)
        .frame(maxHeight: 515)
    }
}
