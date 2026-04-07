import Foundation
import AppKit
import FoundationModels

/// Manages LoRA adapter files for Apple Intelligence training.
/// 
/// LoRA adapters are trained using Apple Intelligence on-device, capturing
/// response patterns and behaviors. While Apple Intelligence itself is not
/// directly selectable as an LLM provider (due to context limitations), the
/// trained adapters can be used to enhance any connected LLM provider.
///
/// Workflow:
/// 1. Export task history as JSONL training data
/// 2. Train using Apple's Python toolkit (developer.apple.com)
/// 3. Install the resulting .fmadapter file
/// 4. The adapter enhances responses from Claude, Ollama, or other providers
@MainActor @Observable
final class LoRAAdapterManager {
    static let shared = LoRAAdapterManager()

    var isLoaded = false
    var adapterName = ""
    var statusMessage = "No adapter loaded"
    var adapterURL: URL?
    var installedAdapters: [URL] = []

    /// The loaded adapter asset (trained with Apple Intelligence).
    private(set) var adapter: SystemLanguageModel.Adapter?

    private static let adapterDir: URL = {
        guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return FileManager.default.temporaryDirectory.appendingPathComponent("Agent/Adapters")
        }
        let dir = appSupport.appendingPathComponent("Agent/Adapters")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }()

    private init() {
        refreshInstalledAdapters()
        autoLoadLastAdapter()
    }

    // MARK: - Install Adapter

    func installAdapter(from sourceURL: URL) -> Bool {
        let name = sourceURL.lastPathComponent
        let destURL = Self.adapterDir.appendingPathComponent(name)

        do {
            if FileManager.default.fileExists(atPath: destURL.path) {
                try FileManager.default.removeItem(at: destURL)
            }
            try FileManager.default.copyItem(at: sourceURL, to: destURL)
            refreshInstalledAdapters()
            loadAdapter(from: destURL)
            return true
        } catch {
            statusMessage = "Install failed: \(error.localizedDescription)"
            return false
        }
    }

    // MARK: - Uninstall

    func uninstallAdapter(at url: URL) {
        try? FileManager.default.removeItem(at: url)
        if adapterURL == url {
            unloadAdapter()
        }
        refreshInstalledAdapters()
        UserDefaults.standard.removeObject(forKey: "loraLastAdapterPath")
    }

    // MARK: - List Installed

    func refreshInstalledAdapters() {
        let contents = (try? FileManager.default.contentsOfDirectory(at: Self.adapterDir, includingPropertiesForKeys: nil)) ?? []
        installedAdapters = contents.filter {
            $0.pathExtension == "fmadapter" || $0.hasDirectoryPath
        }.sorted { $0.lastPathComponent < $1.lastPathComponent }
    }

    // MARK: - Load / Unload

    func loadAdapter(from url: URL) {
        do {
            let loadedAdapter = try SystemLanguageModel.Adapter(fileURL: url)
            self.adapter = loadedAdapter
            self.adapterURL = url
            self.adapterName = url.deletingPathExtension().lastPathComponent
            self.isLoaded = true
            self.statusMessage = "Active: \(adapterName)"
            UserDefaults.standard.set(url.path, forKey: "loraLastAdapterPath")
        } catch {
            self.adapter = nil
            self.isLoaded = false
            self.statusMessage = "Failed: \(error.localizedDescription)"
        }
    }

    func unloadAdapter() {
        adapter = nil
        adapterURL = nil
        adapterName = ""
        isLoaded = false
        statusMessage = "No adapter loaded"
        UserDefaults.standard.removeObject(forKey: "loraLastAdapterPath")
    }

    private func autoLoadLastAdapter() {
        guard let path = UserDefaults.standard.string(forKey: "loraLastAdapterPath") else { return }
        let url = URL(fileURLWithPath: path)
        if FileManager.default.fileExists(atPath: path) {
            loadAdapter(from: url)
        }
    }

    // MARK: - Directories

    static var jsonlDir: URL {
        guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return FileManager.default.temporaryDirectory.appendingPathComponent("Agent/LoRAJsonL")
        }
        let dir = appSupport.appendingPathComponent("Agent/LoRAJsonL")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    static var trainingEnvDir: URL {
        guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return FileManager.default.temporaryDirectory.appendingPathComponent("Agent/TrainingEnv")
        }
        return appSupport.appendingPathComponent("Agent/TrainingEnv")
    }

    static var baseDir: URL {
        guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return FileManager.default.temporaryDirectory.appendingPathComponent("Agent")
        }
        let dir = appSupport.appendingPathComponent("Agent")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    // MARK: - JSONL Export from Task History

    /// Export recent task history as JSONL training data for LoRA fine-tuning.
    func exportTaskHistoryAsJSONL() -> URL? {
        let history = TaskHistory.shared
        let records = history.records

        guard !records.isEmpty else { return nil }

        let encoder = JSONEncoder()
        encoder.outputFormatting = []

        var lines: [String] = []
        for record in records {
            let entry = JSONLEntry(messages: [
                JSONLMessage(role: "user", content: record.prompt),
                JSONLMessage(role: "assistant", content: record.summary)
            ])
            guard let data = try? encoder.encode(entry),
                  let str = String(data: data, encoding: .utf8) else { continue }
            lines.append(str)
        }

        guard !lines.isEmpty else { return nil }

        let content = lines.joined(separator: "\n")
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd_HHmmss"
        let filename = "agent_training_\(formatter.string(from: Date())).jsonl"
        let fileURL = Self.jsonlDir.appendingPathComponent(filename)

        do {
            try content.write(to: fileURL, atomically: true, encoding: .utf8)
            return fileURL
        } catch {
            return nil
        }
    }

    /// Import a JSONL file into the training data directory. Returns sample count.
    func importJSONL(from sourceURL: URL) -> Int {
        guard let content = try? String(contentsOf: sourceURL, encoding: .utf8) else { return 0 }

        // Validate and count entries
        let decoder = JSONDecoder()
        var validCount = 0
        for line in content.components(separatedBy: "\n") where !line.isEmpty {
            guard let data = line.data(using: .utf8),
                  let entry = try? decoder.decode(JSONLEntry.self, from: data),
                  entry.messages.count >= 2 else { continue }
            validCount += 1
        }

        guard validCount > 0 else { return 0 }

        // Copy to our JSONL directory
        let destName = sourceURL.lastPathComponent
        let destURL = Self.jsonlDir.appendingPathComponent(destName)
        do {
            if FileManager.default.fileExists(atPath: destURL.path) {
                try FileManager.default.removeItem(at: destURL)
            }
            try content.write(to: destURL, atomically: true, encoding: .utf8)
        } catch {
            return 0
        }

        return validCount
    }

    /// Delete a saved JSONL file.
    func deleteJSONLFile(at url: URL) {
        try? FileManager.default.removeItem(at: url)
    }

    /// List saved JSONL files.
    func savedFiles() -> [URL] {
        let contents = (try? FileManager.default.contentsOfDirectory(at: Self.jsonlDir, includingPropertiesForKeys: nil)) ?? []
        return contents.filter { $0.pathExtension == "jsonl" }.sorted { $0.lastPathComponent > $1.lastPathComponent }
    }

    // MARK: - Python Environment

    /// Check if Python 3.11+ is available.
    nonisolated static func pythonStatus() async -> (available: Bool, version: String, path: String) {
        await Task.detached { pythonStatusCheck() }.value
    }

    nonisolated private static func pythonStatusCheck() -> (available: Bool, version: String, path: String) {
        let candidates = [
            "/opt/homebrew/bin/python3.13", "/opt/homebrew/bin/python3.12", "/opt/homebrew/bin/python3.11",
            "/usr/local/bin/python3.13", "/usr/local/bin/python3.12", "/usr/local/bin/python3.11",
            "/Library/Frameworks/Python.framework/Versions/3.13/bin/python3",
            "/Library/Frameworks/Python.framework/Versions/3.12/bin/python3",
            "/Library/Frameworks/Python.framework/Versions/3.11/bin/python3",
            "/opt/homebrew/bin/python3", "/usr/local/bin/python3", "/usr/bin/python3"
        ]

        for path in candidates {
            guard FileManager.default.fileExists(atPath: path) else { continue }
            let p = Process()
            let pipe = Pipe()
            p.executableURL = URL(fileURLWithPath: path)
            p.arguments = ["--version"]
            p.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
            var pyEnv = ProcessInfo.processInfo.environment
            pyEnv["HOME"] = NSHomeDirectory()
            let pyPaths = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
            pyEnv["PATH"] = pyPaths + ":" + (pyEnv["PATH"] ?? "")
            p.environment = pyEnv
            p.standardOutput = pipe
            p.standardError = pipe
            try? p.run()
            p.waitUntilExit()
            let output = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            if let range = output.range(of: #"\d+\.\d+"#, options: .regularExpression) {
                let ver = String(output[range])
                let parts = ver.split(separator: ".")
                if parts.count >= 2, let major = Int(parts[0]), let minor = Int(parts[1]),
                   major >= 3, minor >= 11 {
                    return (true, output, path)
                }
            }
        }
        return (false, "Not found (need 3.11+)", "")
    }

    /// Check if virtual environment exists.
    static func venvExists() -> Bool {
        FileManager.default.fileExists(atPath: trainingEnvDir.appendingPathComponent("bin/python3").path)
    }

    // MARK: - Setup Scripts

    static func generateSetupScript(homebrew: Bool) -> URL? {
        let scriptName = homebrew ? "setup_training_homebrew.sh" : "setup_training_direct.sh"
        let scriptURL = baseDir.appendingPathComponent(scriptName)
        let envPath = trainingEnvDir.path
        let jsonlPath = jsonlDir.path
        let adapterPath = adapterDir.path
        let toolkitDir = "/path/to/apple-adapter-toolkit"

        let pythonSetup: String
        if homebrew {
            pythonSetup = """
            # Step 1: Check for Homebrew
            if ! command -v brew &>/dev/null; then
                echo "Installing Homebrew..."
                /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
                eval "$(/opt/homebrew/bin/brew shellenv)"
            fi
            echo "Homebrew found"

            # Step 2: Install Python 3.11
            if ! command -v python3.11 &>/dev/null; then
                echo "Installing Python 3.11..."
                brew install python@3.11
            fi
            PYTHON=python3.11
            echo "Python: $($PYTHON --version)"
            """
        } else {
            pythonSetup = """
            # Step 1: Find Python 3.11+
            PYTHON=""
            for candidate in \\
                /Library/Frameworks/Python.framework/Versions/3.13/bin/python3 \\
                /Library/Frameworks/Python.framework/Versions/3.12/bin/python3 \\
                /Library/Frameworks/Python.framework/Versions/3.11/bin/python3 \\
                /opt/homebrew/bin/python3.13 /opt/homebrew/bin/python3.12 /opt/homebrew/bin/python3.11 \\
                /usr/local/bin/python3.13 /usr/local/bin/python3.12 /usr/local/bin/python3.11 \\
                /usr/local/bin/python3 /usr/bin/python3; do
                if [ -x "$candidate" ]; then
                    VER=$("$candidate" --version 2>&1 | grep -oE '[0-9]+\\.[0-9]+')
                    MAJOR=$(echo "$VER" | cut -d. -f1)
                    MINOR=$(echo "$VER" | cut -d. -f2)
                    if [ "$MAJOR" -ge 3 ] && [ "$MINOR" -ge 11 ]; then
                        PYTHON="$candidate"
                        break
                    fi
                fi
            done
            if [ -z "$PYTHON" ]; then
                echo "ERROR: Python 3.11+ not found. Install from https://www.python.org/downloads/"
                exit 1
            fi
            echo "Python found: $PYTHON ($($PYTHON --version))"
            """
        }

        let script = """
        #!/bin/bash
        # Agent LoRA Training Environment Setup
        set -e
        echo "=== Agent LoRA Training Setup ==="
        echo ""

        \(pythonSetup)

        # Create virtual environment
        VENV="\(envPath)"
        if [ ! -d "$VENV" ]; then
            echo "Creating virtual environment..."
            "$PYTHON" -m venv "$VENV"
        fi
        echo "Virtual environment ready"

        source "$VENV/bin/activate"
        pip install --upgrade pip

        # Install toolkit
        TOOLKIT="\(toolkitDir)"
        if [ -f "$TOOLKIT/requirements.txt" ]; then
            pip install -r "$TOOLKIT/requirements.txt"
            echo "Toolkit dependencies installed"
        else
            echo "Toolkit not found at $TOOLKIT"
            echo "Download from: https://developer.apple.com/download/foundation-models-adapter/"
        fi

        echo ""
        echo "=== Setup Complete ==="

        # Train if data exists
        TRAIN_FILE=$(ls -t "\(jsonlPath)"/*.jsonl 2>/dev/null | head -1)
        if [ -n "$TRAIN_FILE" ] && [ -d "$TOOLKIT" ]; then
            echo ""
            echo "=== Training Adapter ==="
            cd "$TOOLKIT"
            python -m examples.train_adapter \\
              --train-data "$TRAIN_FILE" \\
              --epochs 10 --learning-rate 1e-3 --batch-size 4 \\
              --checkpoint-dir "\(adapterPath)/checkpoints/"
            echo ""
            echo "=== Exporting Adapter ==="
            python -m examples.export_adapter \\
              --checkpoint-dir "\(adapterPath)/checkpoints/" \\
              --output "\(adapterPath)/Agent.fmadapter"
            echo ""
            echo "Adapter saved to: \(adapterPath)/Agent.fmadapter"
        else
            echo ""
            echo "To train:"
            echo "  source \(envPath)/bin/activate"
            echo "  cd $TOOLKIT"
            echo "  python -m examples.train_adapter --train-data \(jsonlPath)/your_file.jsonl --epochs 10 --batch-size 4 --learning-rate 1e-3 --checkpoint-dir \(adapterPath)/checkpoints/"
            echo "  python -m examples.export_adapter --checkpoint-dir \(adapterPath)/checkpoints/ --output \(adapterPath)/Agent.fmadapter"
        fi
        """

        do {
            try script.write(to: scriptURL, atomically: true, encoding: .utf8)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: scriptURL.path)
            return scriptURL
        } catch {
            return nil
        }
    }

    /// Open Terminal at the Agent support directory.
    static func openTerminal() {
        let path = baseDir.path
        Task.detached {
            let script = "tell application \"Terminal\" to do script \"cd '\(path)' && ls -la\""
            if let appleScript = NSAppleScript(source: script) {
                var error: NSDictionary?
                appleScript.executeAndReturnError(&error)
            }
        }
    }

    /// Reveal training folder in Finder.
    static func revealInFinder() {
        NSWorkspace.shared.open(baseDir)
    }
}

// MARK: - JSONL Codable Helpers

private struct JSONLMessage: Codable {
    let role: String
    let content: String
}

private struct JSONLEntry: Codable {
    let messages: [JSONLMessage]
}
