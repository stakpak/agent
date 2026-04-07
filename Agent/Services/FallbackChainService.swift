import Foundation
import AgentTools

/// A single entry in the model fallback chain.
struct FallbackEntry: Codable, Identifiable {
    let id: UUID
    var provider: String // APIProvider.rawValue
    var model: String
    var enabled: Bool

    init(provider: String, model: String, enabled: Bool = true) {
        self.id = UUID()
        self.provider = provider
        self.model = model
        self.enabled = enabled
    }

    var displayName: String {
        let p = APIProvider(rawValue: provider)?.displayName ?? provider
        return "\(p) / \(model)"
    }
}

/// Manages a user-configured fallback chain for overnight unattended runs.
/// When the primary provider/model fails N times, automatically switches to the next.
/// Persisted to UserDefaults.
@MainActor
@Observable
final class FallbackChainService {
    static let shared = FallbackChainService()

    private static let udKey = "agent.fallbackChain"
    private static let udEnabledKey = "agent.fallbackEnabled"

    /// Max consecutive failures before falling to next provider.
    static let maxFailuresBeforeFallback = 3

    /// The ordered fallback chain. Entry 0 is tried after the primary fails.
    private(set) var chain: [FallbackEntry] = [] {
        didSet { save() }
    }

    /// Whether fallback is enabled at all.
    var enabled: Bool = UserDefaults.standard.bool(forKey: udEnabledKey) {
        didSet { UserDefaults.standard.set(enabled, forKey: Self.udEnabledKey) }
    }

    /// Current position in the fallback chain (0 = first fallback, -1 = still on primary).
    var currentIndex: Int = -1

    /// Consecutive failure count for the current provider.
    var consecutiveFailures: Int = 0

    private init() {
        load()
    }

    // MARK: - Chain Management

    func add(provider: String, model: String) {
        chain.append(FallbackEntry(provider: provider, model: model))
    }

    func remove(id: UUID) {
        chain.removeAll { $0.id == id }
    }

    func toggle(id: UUID) {
        if let idx = chain.firstIndex(where: { $0.id == id }) {
            chain[idx].enabled.toggle()
        }
    }

    func move(from: IndexSet, to: Int) {
        chain.move(fromOffsets: from, toOffset: to)
    }

    func clear() {
        chain.removeAll()
        currentIndex = -1
        consecutiveFailures = 0
    }

    // MARK: - Fallback Logic

    /// Record a successful API call — resets failure count AND chain position so the
    /// next call goes back to the primary provider. This allows the chain to recover
    /// from a transient failure (e.g., temporary rate limit) instead of being stuck
    /// on the fallback provider for the rest of the session.
    func recordSuccess() {
        consecutiveFailures = 0
        currentIndex = -1
    }

    /// Record a failure. Returns the next fallback entry if threshold reached, nil if no more fallbacks.
    func recordFailure() -> FallbackEntry? {
        consecutiveFailures += 1
        guard enabled, consecutiveFailures >= Self.maxFailuresBeforeFallback else { return nil }

        // Move to next in chain
        let nextIndex = currentIndex + 1
        let enabledChain = chain.filter { $0.enabled }
        guard nextIndex < enabledChain.count else { return nil }

        currentIndex = nextIndex
        consecutiveFailures = 0
        return enabledChain[nextIndex]
    }

    /// Reset fallback state (call at task start).
    func reset() {
        currentIndex = -1
        consecutiveFailures = 0
    }

    /// The currently active fallback entry, or nil if still on primary.
    var activeFallback: FallbackEntry? {
        guard enabled, currentIndex >= 0 else { return nil }
        let enabledChain = chain.filter { $0.enabled }
        guard currentIndex < enabledChain.count else { return nil }
        return enabledChain[currentIndex]
    }

    /// Summary for display.
    var summary: String {
        guard !chain.isEmpty else { return "No fallback chain configured." }
        let enabledChain = chain.filter { $0.enabled }
        return enabledChain.enumerated().map { i, entry in
            let marker = i == currentIndex ? "→ " : "  "
            return "\(marker)\(i + 1). \(entry.displayName)"
        }.joined(separator: "\n")
    }

    // MARK: - Persistence

    private func load() {
        guard let data = UserDefaults.standard.data(forKey: Self.udKey),
              let decoded = try? JSONDecoder().decode([FallbackEntry].self, from: data) else { return }
        chain = decoded
    }

    private func save() {
        guard let data = try? JSONEncoder().encode(chain) else { return }
        UserDefaults.standard.set(data, forKey: Self.udKey)
    }
}
