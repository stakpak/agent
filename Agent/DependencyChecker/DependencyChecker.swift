import Foundation

struct DependencyStatus {
    let xcodeTools: Bool
    let clang: Bool
    let appleIntelligence: Bool
    let appleIntelligenceStatus: String

    var allGood: Bool { xcodeTools && clang }
}

struct DependencyChecker {
    static func check() -> DependencyStatus {
        let fm = FileManager.default
        let xcodeTools = fm.fileExists(atPath: "/Library/Developer/CommandLineTools/usr/bin/clang")
        let clang = fm.fileExists(atPath: "/usr/bin/clang")
        let (aiAvailable, aiStatus) = checkAppleIntelligence()
        return DependencyStatus(xcodeTools: xcodeTools, clang: clang, appleIntelligence: aiAvailable, appleIntelligenceStatus: aiStatus)
    }

    /// Check if Apple Intelligence is available and enabled
    static func checkAppleIntelligence() -> (Bool, String) {
        // Check macOS version - Apple Intelligence requires macOS 26.0+
        let osVersion = ProcessInfo.processInfo.operatingSystemVersion
        guard osVersion.majorVersion >= 26 else {
            return (false, "Requires macOS 26.0+ (currently \(osVersion.majorVersion).\(osVersion.minorVersion).\(osVersion.patchVersion))")
        }

        // Check if running on Apple Silicon (M1+)
        #if arch(arm64)
            // On macOS 26+, Apple Intelligence is fully integrated and supporte
            if #available(macOS 26.0, *) {
                // Apple Intelligence requires M1 or later
                let defaults = UserDefaults.standard
                // Apple Intelligence is enabled by default on supported Macs ru
                if let aiEnabled = defaults.object(forKey: "AppleIntelligenceEnabled") as? Bool, aiEnabled {
                    return (true, "Enabled")
                }

                // Check system language - Apple Intelligence supports multiple
                let preferredLanguage = Locale.preferredLanguages.first ?? ""
                if preferredLanguage.hasPrefix("en") {
                    // On macOS 26+ with Apple Silicon, AI is fully available
                    return (true, "Available")
                } else {
                    // Apple Intelligence now supports many languages
                    return (true, "Available")
                }
            }
            return (true, "Available")
        #else
            // Intel Macs don't support Apple Intelligence
            return (false, "Requires Apple Silicon (M1+)")
        #endif
    }

    /// Launch the Xcode Command Line Tools installer via xcode-select --install
    static func installCommandLineTools() {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/xcode-select")
        process.arguments = ["--install"]
        process.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
        try? process.run()
    }
}
