import Foundation
import Security
import AgentAudit

/// Secure credential storage using the macOS data protection keychain.
final class KeychainService: Sendable {
    static let shared = KeychainService()

    private init() {}

    private static let claudeAPIKey = "agent.claudeAPIKey"
    private static let ollamaAPIKey = "agent.ollamaAPIKey"
    private static let tavilyAPIKey = "agent.tavilyAPIKey"
    private static let openAIAPIKey = "agent.openAIAPIKey"
    private static let deepSeekAPIKey = "agent.deepSeekAPIKey"
    private static let huggingFaceAPIKey = "agent.huggingFaceAPIKey"
    private static let vLLMAPIKey = "agent.vLLMAPIKey"

    func setClaudeAPIKey(_ key: String) { set(key: Self.claudeAPIKey, value: key) }
    func getClaudeAPIKey() -> String? { get(key: Self.claudeAPIKey) }

    func setOllamaAPIKey(_ key: String) { set(key: Self.ollamaAPIKey, value: key) }
    func getOllamaAPIKey() -> String? { get(key: Self.ollamaAPIKey) }

    func setTavilyAPIKey(_ key: String) { set(key: Self.tavilyAPIKey, value: key) }
    func getTavilyAPIKey() -> String? { get(key: Self.tavilyAPIKey) }

    func setOpenAIAPIKey(_ key: String) { set(key: Self.openAIAPIKey, value: key) }
    func getOpenAIAPIKey() -> String? { get(key: Self.openAIAPIKey) }

    func setDeepSeekAPIKey(_ key: String) { set(key: Self.deepSeekAPIKey, value: key) }
    func getDeepSeekAPIKey() -> String? { get(key: Self.deepSeekAPIKey) }

    func setHuggingFaceAPIKey(_ key: String) { set(key: Self.huggingFaceAPIKey, value: key) }
    func getHuggingFaceAPIKey() -> String? { get(key: Self.huggingFaceAPIKey) }

    func setVLLMAPIKey(_ key: String) { set(key: Self.vLLMAPIKey, value: key) }
    func getVLLMAPIKey() -> String? { get(key: Self.vLLMAPIKey) }

    private static let zAIAPIKey = "com.agent.zai-api-key"
    func setZAIAPIKey(_ key: String) { set(key: Self.zAIAPIKey, value: key) }
    func getZAIAPIKey() -> String? { get(key: Self.zAIAPIKey) }

    private static let geminiAPIKey = "com.agent.gemini-api-key"
    func setGeminiAPIKey(_ key: String) { set(key: Self.geminiAPIKey, value: key) }
    func getGeminiAPIKey() -> String? { get(key: Self.geminiAPIKey) }

    private static let grokAPIKey = "com.agent.grok-api-key"
    func setGrokAPIKey(_ key: String) { set(key: Self.grokAPIKey, value: key) }
    func getGrokAPIKey() -> String? { get(key: Self.grokAPIKey) }

    private static let mistralAPIKey = "com.agent.mistral-api-key"
    func setMistralAPIKey(_ key: String) { set(key: Self.mistralAPIKey, value: key) }
    func getMistralAPIKey() -> String? { get(key: Self.mistralAPIKey) }

    private static let codestralAPIKey = "com.agent.codestral-api-key"
    func setCodestralAPIKey(_ key: String) { set(key: Self.codestralAPIKey, value: key) }
    func getCodestralAPIKey() -> String? { get(key: Self.codestralAPIKey) }

    private static let vibeAPIKeyId = "com.agent.vibe-api-key"
    func setVibeAPIKey(_ key: String) { set(key: Self.vibeAPIKeyId, value: key) }
    func getVibeAPIKey() -> String? { get(key: Self.vibeAPIKeyId) }

    private static let bigModelAPIKeyId = "com.agent.bigmodel-api-key"
    func setBigModelAPIKey(_ key: String) { set(key: Self.bigModelAPIKeyId, value: key) }
    func getBigModelAPIKey() -> String? { get(key: Self.bigModelAPIKeyId) }

    private static let qwenAPIKeyId = "com.agent.qwen-api-key"
    func setQwenAPIKey(_ key: String) { set(key: Self.qwenAPIKeyId, value: key) }
    func getQwenAPIKey() -> String? { get(key: Self.qwenAPIKeyId) }

    private func set(key: String, value: String) {
        guard let data = value.data(using: .utf8) else { return }
        delete(key: key)

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecAttrService as String: "Agent!",
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlocked,
            kSecUseDataProtectionKeychain as String: true
        ]

        let status = SecItemAdd(query as CFDictionary, nil)
        if status != errSecSuccess && status != errSecDuplicateItem {
            AuditLog.log(.keychain, "KeychainService: Failed to store \(key): \(status)")
        }
    }

    private func get(key: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecAttrService as String: "Agent!",
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
            kSecUseDataProtectionKeychain as String: true
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        guard status == errSecSuccess,
              let data = result as? Data,
              let value = String(data: data, encoding: .utf8) else
        {
            return nil
        }
        return value
    }

    private func delete(key: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecAttrService as String: "Agent!",
            kSecUseDataProtectionKeychain as String: true
        ]
        SecItemDelete(query as CFDictionary)
    }
}
