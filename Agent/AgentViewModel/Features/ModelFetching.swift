@preconcurrency import Foundation
import AppKit
import SwiftUI
import AgentAudit
import AgentTools

// MARK: - Model Fetching Extension
extension AgentViewModel {

    func fetchClaudeModels() async {
        guard !apiKey.isEmpty else {
            await MainActor.run {
                self.availableClaudeModels = Self.defaultClaudeModels
            }
            return
        }

        do {
            let models = try await Self.fetchClaudeModelsFromAPI(apiKey: apiKey)
            await MainActor.run {
                self.availableClaudeModels = models.isEmpty ? Self.defaultClaudeModels : models
            }
        } catch {
            AuditLog.log(.api, "Error fetching Claude models: \(error)")
            await MainActor.run {
                self.availableClaudeModels = Self.defaultClaudeModels
            }
        }
    }

    private static func fetchClaudeModelsFromAPI(apiKey: String) async throws -> [ClaudeModelInfo] {
        guard let url = URL(string: "https://api.anthropic.com/v1/models") else
            throw AgentError.invalidURL
        }

        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue(apiKey, forHTTPHeaderField: "x-api-key")
        request.setValue("2023-06-01", forHTTPHeaderField: "anthropic-version")
        request.timeoutInterval = llmAPITimeout

        let (data, response) = try await URLSession.shared.data(for: request)

        guard let httpResponse = response as? HTTPURLResponse,
              httpResponse.statusCode == 200 else
        {
            throw AgentError.apiError(statusCode: (response as? HTTPURLResponse)?.statusCode ?? 0, message: "API error")
        }

        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let modelsData = json["data"] as? [[String: Any]] else
        {
            return defaultClaudeModels
        }

        let models = modelsData.compactMap { modelData -> ClaudeModelInfo? in
            guard let id = modelData["id"] as? String else { return nil }
            let displayName = modelData["display_name"] as? String ?? id
            let createdAt = modelData["created_at"] as? String
            let description = modelData["description"] as? String

            return ClaudeModelInfo(
                id: id,
                name: displayName,
                displayName: displayName,
                createdAt: createdAt,
                description: description
            )
        }

        return models.isEmpty ? defaultClaudeModels : models
    }

    func fetchOllamaModels() {
        let endpoint = ollamaEndpoint
        let apiKey = ollamaAPIKey
        isFetchingModels = true
        Task {
            defer { isFetchingModels = false }
            do {
                let models = try await Self.fetchModels(endpoint: endpoint, apiKey: apiKey)
                ollamaModels = models.isEmpty ? Self.defaultOllamaModels : models
                // Auto-select first model if current selection is empty or not
                let names = ollamaModels.map(\.name)
                if ollamaModel.isEmpty || (!names.isEmpty && !names.contains(ollamaModel)) {
                    ollamaModel = names.first ?? ""
                }
            } catch {
                appendLog("Failed to fetch models: \(error.localizedDescription)")
                ollamaModels = Self.defaultOllamaModels
            }
        }
    }

    func fetchLocalOllamaModels() {
        let endpoint = localOllamaEndpoint
        isFetchingLocalModels = true
        Task {
            defer { isFetchingLocalModels = false }
            do {
                let models = try await Self.fetchModels(endpoint: endpoint, apiKey: "")
                localOllamaModels = models.isEmpty ? Self.defaultOllamaModels : models
                let names = localOllamaModels.map(\.name)
                if localOllamaModel.isEmpty || (!names.isEmpty && !names.contains(localOllamaModel)) {
                    localOllamaModel = names.first ?? ""
                }
            } catch {
                appendLog("Failed to fetch local models: \(error.localizedDescription)")
                localOllamaModels = Self.defaultOllamaModels
            }
        }
    }

    // MARK: - OpenAI Model Fetching

    func fetchOpenAIModels() {
        guard !openAIAPIKey.isEmpty else {
            openAIModels = Self.defaultOpenAIModels
            return
        }
        isFetchingOpenAIModels = true
        Task {
            defer { isFetchingOpenAIModels = false }
            do {
                let models = try await Self.fetchOpenAIModelsFromAPI(apiKey: openAIAPIKey)
                openAIModels = models.isEmpty ? Self.defaultOpenAIModels : models
                let ids = openAIModels.map(\.id)
                if openAIModel.isEmpty || (!ids.isEmpty && !ids.contains(openAIModel)) {
                    openAIModel = ids.first ?? ""
                }
            } catch {
                appendLog("Failed to fetch OpenAI models: \(error.localizedDescription)")
                openAIModels = Self.defaultOpenAIModels
            }
        }
    }

    func fetchDeepSeekModels() {
        guard !deepSeekAPIKey.isEmpty else {
            deepSeekModels = Self.defaultDeepSeekModels
            return
        }
        isFetchingDeepSeekModels = true
        Task {
            defer { isFetchingDeepSeekModels = false }
            do {
                let models = try await Self.fetchOpenAICompatibleModels(
                    baseURL: "https://api.deepseek.com/v1",
                    apiKey: deepSeekAPIKey
                )
                deepSeekModels = models.isEmpty ? Self.defaultDeepSeekModels : models
                let ids = deepSeekModels.map(\.id)
                if deepSeekModel.isEmpty || (!ids.isEmpty && !ids.contains(deepSeekModel)) {
                    deepSeekModel = ids.first ?? ""
                }
            } catch {
                appendLog("Failed to fetch DeepSeek models: \(error.localizedDescription)")
                deepSeekModels = Self.defaultDeepSeekModels
            }
        }
    }

    func fetchHuggingFaceModels() {
        guard !huggingFaceAPIKey.isEmpty else {
            huggingFaceModels = Self.defaultHuggingFaceModels
            return
        }
        isFetchingHuggingFaceModels = true
        Task {
            defer { isFetchingHuggingFaceModels = false }
            do {
                let models = try await Self.fetchHuggingFaceModelsFromAPI(apiKey: huggingFaceAPIKey)
                huggingFaceModels = models.isEmpty ? Self.defaultHuggingFaceModels : models
                let ids = huggingFaceModels.map(\.id)
                if huggingFaceModel.isEmpty || (!ids.isEmpty && !ids.contains(huggingFaceModel)) {
                    huggingFaceModel = ids.first ?? ""
                }
            } catch {
                appendLog("Failed to fetch HuggingFace models: \(error.localizedDescription)")
                huggingFaceModels = Self.defaultHuggingFaceModels
            }
        }
    }

    // MARK: - Static API Fetch Helpers

    private nonisolated static func fetchOpenAIModelsFromAPI(apiKey: String) async throws -> [OpenAIModelInfo] {
        guard let url = URL(string: "https://api.openai.com/v1/models") else {
            throw AgentError.invalidURL
        }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = llmAPITimeout

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw AgentError.apiError(statusCode: (response as? HTTPURLResponse)?.statusCode ?? 0, message: "OpenAI API error")
        }

        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let modelsArray = json["data"] as? [[String: Any]] else
        {
            return defaultOpenAIModels
        }

        let filtered = modelsArray
            .filter { model in
                let id = model["id"] as? String ?? ""
                return id.hasPrefix("gpt-") || id.hasPrefix("chatgpt-") || id.hasPrefix("o1-") || id.hasPrefix("o3-") || id.hasPrefix("o4-")
            }
            .compactMap { model -> OpenAIModelInfo? in
                guard let id = model["id"] as? String else { return nil }
                return OpenAIModelInfo(id: id, name: id)
            }
            .sorted { $0.name < $1.name }

        return filtered.isEmpty ? defaultOpenAIModels : filtered
    }

    private nonisolated static func fetchOpenAICompatibleModels(baseURL: String, apiKey: String) async throws -> [OpenAIModelInfo] {
        let endpoint = baseURL.hasSuffix("/models") ? baseURL : baseURL + "/models"
        guard let url = URL(string: endpoint) else { throw AgentError.invalidURL }

        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = llmAPITimeout

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw AgentError.apiError(statusCode: (response as? HTTPURLResponse)?.statusCode ?? 0, message: "API error")
        }

        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let modelsArray = json["data"] as? [[String: Any]] else
        {
            return defaultDeepSeekModels
        }

        let models = modelsArray.compactMap { model -> OpenAIModelInfo? in
            guard let id = model["id"] as? String else { return nil }
            return OpenAIModelInfo(id: id, name: id)
        }.sorted { $0.name < $1.name }

        return models.isEmpty ? defaultDeepSeekModels : models
    }

    private nonisolated static func fetchHuggingFaceModelsFromAPI(apiKey: String) async throws -> [OpenAIModelInfo] {
        // Use the router endpoint which returns inference-ready models (OpenAI-
        guard let url = URL(string: "https://router.huggingface.co/v1/models") e
            throw AgentError.invalidURL
        }

        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = llmAPITimeout

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
            throw AgentError.apiError(statusCode: (response as? HTTPURLResponse)?.statusCode ?? 0, message: "HuggingFace API error")
        }

        // Router returns OpenAI-compatible format: {"data": [{"id": "model-id",
        if let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
           let dataArray = json["data"] as? [[String: Any]]
        {
            let models = dataArray.compactMap { model -> OpenAIModelInfo? in
                guard let id = model["id"] as? String else { return nil }
                // Use last path component as display name
                let name = id.components(separatedBy: "/").last ?? id
                return OpenAIModelInfo(id: id, name: name)
            }.sorted { $0.name < $1.name }
            return models
        }

        // Fallback: old format (array of objects)
        if let json = try JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
            let models = json.compactMap { model -> OpenAIModelInfo? in
                guard let id = model["id"] as? String else { return nil }
                return OpenAIModelInfo(id: id, name: id)
            }.sorted { $0.name < $1.name }
            return models
        }

        return defaultHuggingFaceModels
    }

    private nonisolated static func fetchModels(endpoint: String, apiKey: String) async throws -> [OllamaModelInfo] {
        let effectiveEndpoint = endpoint.isEmpty ? "http://localhost:11434/api/c
        guard let chatURL = URL(string: effectiveEndpoint) else { throw AgentError.invalidResponse }
        let baseDir = chatURL.deletingLastPathComponent().absoluteString

        guard let tagsURL = URL(string: baseDir + "tags") else { throw AgentError.invalidResponse }
        guard let showURL = URL(string: baseDir + "show") else { throw AgentError.invalidResponse }

        // 1. Fetch model list
        var tagsRequest = URLRequest(url: tagsURL)
        tagsRequest.httpMethod = "GET"
        tagsRequest.setValue("application/json", forHTTPHeaderField: "content-type")
        if !apiKey.isEmpty {
            tagsRequest.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        }
        tagsRequest.timeoutInterval = llmAPITimeout

        let (data, response) = try await URLSession.shared.data(for: tagsRequest)

        guard let httpResponse = response as? HTTPURLResponse,
              httpResponse.statusCode == 200 else
        {
            let errorBody = String(data: data, encoding: .utf8) ?? "Unknown error"
            throw AgentError.apiError(statusCode: (response as? HTTPURLResponse)?.statusCode ?? 0, message: errorBody)
        }

        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let models = json["models"] as? [[String: Any]] else
        {
            throw AgentError.invalidResponse
        }

        let names = models.compactMap { $0["name"] as? String }.sorted()

        // 2. Check capabilities for each model via /api/show (in parallel)
        return await withTaskGroup(of: OllamaModelInfo?.self) { group in
            for name in names {
                group.addTask {
                    let hasVision = await Self.checkVision(model: name, showURL: showURL, apiKey: apiKey)
                    return OllamaModelInfo(id: name, name: name, supportsVision: hasVision)
                }
            }
            var results: [OllamaModelInfo] = []
            for await info in group {
                if let info { results.append(info) }
            }
            return results.sorted { $0.name < $1.name }
        }
    }

    /// Check if a model has "vision" in its capabilities via /api/show
    private nonisolated static func checkVision(model: String, showURL: URL, apiKey: String) async -> Bool {
        do {
            let body = try JSONSerialization.data(withJSONObject: ["model": model])
            var request = URLRequest(url: showURL)
            request.httpMethod = "POST"
            request.setValue("application/json", forHTTPHeaderField: "content-type")
            if !apiKey.isEmpty {
                request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
            }
            request.httpBody = body
            request.timeoutInterval = llmAPITimeout

            let (data, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse, http.statusCode == 200,
                  let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let capabilities = json["capabilities"] as? [String] else
            {
                return false
            }
            return capabilities.contains("vision")
        } catch {
            return false
        }
    }

    // MARK: - Z.ai Models

    func fetchZAIModels() {
        isFetchingZAIModels = true
        let key = zAIAPIKey
        Task {
            defer { isFetchingZAIModels = false }
            guard !key.isEmpty else { return }
            let models = await Self.fetchZAIModelsFromAPI(apiKey: key)
            if !models.isEmpty {
                zAIModels = models
                if zAIModel.isEmpty || !zAIModels.contains(where: { $0.id == zAIModel }) {
                    zAIModel = zAIModels.first?.id ?? ""
                }
            }
        }
    }

    private nonisolated static func fetchZAIModelsFromAPI(apiKey: String) async -> [OpenAIModelInfo] {
        // Fetch ALL models dynamically from Z.ai's OpenAPI spec The /models end
        let coding = (try? await fetchZAIEndpoint(apiKey: apiKey, urlString: "ht
        let specModels = await fetchZAIModelsFromSpec()

        var seen = Set<String>()
        var result: [OpenAIModelInfo] = []

        // Coding models first (no suffix — use coding endpoint, name tagged -Co
        for m in coding {
            if seen.insert(m.id).inserted {
                result.append(OpenAIModelInfo(id: m.id, name: "\(m.name)-Code"))
            }
        }
        // All spec models — text as coding, vision/image/audio as :v
        for m in specModels {
            if seen.insert(m.id).inserted { result.append(m) }
        }
        return result
    }

    /// Fetch all Z.ai model IDs from the OpenAPI spec at docs.z.ai/openapi.json
    private nonisolated static func fetchZAIModelsFromSpec() async -> [OpenAIModelInfo] {
        guard let url = URL(string: "https://docs.z.ai/openapi.json") else { ret
        guard let (data, _) = try? await URLSession.shared.data(from: url),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let components = json["components"] as? [String: Any],
              let schemas = components["schemas"] as? [String: Any] else { return [] }

        var seen = Set<String>()
        var result: [OpenAIModelInfo] = []

        // Text models (coding endpoint)
        let textSchemas = ["ChatCompletionTextRequest"]
        // Vision/non-coding models (general endpoint, tagged :v)
        let visionSchemas = [
            "ChatCompletionVisionRequest",
            "CreateImageRequest",
            "AsyncCreateImageRequest",
            "LayoutParsingRequest",
            "AudioTranscriptionRequest"
        ]

        for name in textSchemas {
            if let schema = schemas[name] as? [String: Any],
               let props = schema["properties"] as? [String: Any],
               let model = props["model"] as? [String: Any],
               let enums = model["enum"] as? [String]
            {
                for id in enums {
                    if seen.insert(id).inserted {
                        result.append(OpenAIModelInfo(id: id, name: id))
                    }
                }
            }
        }
        for name in visionSchemas {
            if let schema = schemas[name] as? [String: Any],
               let props = schema["properties"] as? [String: Any],
               let model = props["model"] as? [String: Any],
               let enums = model["enum"] as? [String]
            {
                for id in enums {
                    let vid = "\(id):v"
                    if seen.insert(vid).inserted {
                        result.append(OpenAIModelInfo(id: vid, name: id))
                    }
                }
            }
        }
        return result
    }

    private nonisolated static func fetchZAIEndpoint(apiKey: String, urlString: String) async throws -> [OpenAIModelInfo] {
        guard let url = URL(string: urlString) else { return [] }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = llmAPITimeout
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200 else { return [] }
        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { return [] }
        let modelsData: [[String: Any]]
        if let d = json["data"] as? [[String: Any]] { modelsData = d }
        else if let m = json["models"] as? [[String: Any]] { modelsData = m }
        else { return [] }
        return modelsData.compactMap { model -> OpenAIModelInfo? in
            guard let id = model["id"] as? String else { return nil }
            return OpenAIModelInfo(id: id, name: id)
        }.sorted { $0.name < $1.name }
    }

    // MARK: - Qwen (DashScope) Models

    func fetchQwenModels() {
        isFetchingQwenModels = true
        let key = qwenAPIKey
        Task {
            defer { isFetchingQwenModels = false }
            guard !key.isEmpty else {
                qwenModels = Self.defaultQwenModels
                return
            }
            // Try international endpoint first, then China mainland
            let endpoints = [
                "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/models",
                "https://dashscope.aliyuncs.com/compatible-mode/v1/models",
            ]
            for endpoint in endpoints {
                do {
                    let models = try await Self.fetchOpenAICompatibleModels(apiKey: key, endpoint: endpoint)
                    if !models.isEmpty {
                        // Filter to chat/reasoning models (skip embedding, tts,
                        let chatModels = models.filter { id in
                            let lower = id.id.lowercased()
                            let skip = [
                                "embed",
                                "tts",
                                "asr",
                                "rerank",
                                "paraformer",
                                "sambert",
                                "cosyvoice",
                                "sensevoice",
                                "farui",
                                "wanx",
                                "flux"
                            ]
                            return !skip.contains(where: { lower.contains($0) })
                        }
                        qwenModels = chatModels.isEmpty ? models : chatModels
                        if qwenModel.isEmpty || !qwenModels.contains(where: { $0.id == qwenModel }) {
                            qwenModel = qwenModels.first?.id ?? "qwen-plus"
                        }
                        return
                    }
                } catch {
                    AuditLog.log(.api, "Failed to fetch Qwen models from \(endpoint): \(error.localizedDescription)")
                }
            }
            qwenModels = Self.defaultQwenModels
        }
    }

    // MARK: - Google Gemini Models

    func fetchGeminiModels() {
        isFetchingGeminiModels = true
        let key = geminiAPIKey
        Task {
            defer { isFetchingGeminiModels = false }
            guard !key.isEmpty else {
                geminiModels = Self.defaultGeminiModels
                return
            }
            do {
                let models = try await Self.fetchOpenAICompatibleModels(
                    apiKey: key,
                    endpoint: "https://generativelanguage.googleapis.com/v1beta/
                )
                geminiModels = models.isEmpty ? Self.defaultGeminiModels : models
                if geminiModel.isEmpty || !geminiModels.contains(where: { $0.id == geminiModel }) {
                    geminiModel = geminiModels.first?.id ?? "gemini-2.5-flash"
                }
            } catch {
                appendLog("Failed to fetch Gemini models: \(error.localizedDescription)")
                geminiModels = Self.defaultGeminiModels
            }
        }
    }

    // MARK: - Grok Models

    func fetchGrokModels() {
        isFetchingGrokModels = true
        let key = grokAPIKey
        Task {
            defer { isFetchingGrokModels = false }
            guard !key.isEmpty else {
                grokModels = Self.defaultGrokModels
                return
            }
            do {
                let models = try await Self.fetchOpenAICompatibleModels(apiKey:
                grokModels = models.isEmpty ? Self.defaultGrokModels : models
                if grokModel.isEmpty || !grokModels.contains(where: { $0.id == grokModel }) {
                    grokModel = grokModels.first?.id ?? "grok-3-mini-fast"
                }
            } catch {
                appendLog("Failed to fetch Grok models: \(error.localizedDescription)")
                grokModels = Self.defaultGrokModels
            }
        }
    }

    // MARK: - Mistral Models

    func fetchMistralModels() {
        isFetchingMistralModels = true
        let key = mistralAPIKey
        Task {
            defer { isFetchingMistralModels = false }
            guard !key.isEmpty else {
                mistralModels = Self.defaultMistralModels
                return
            }
            do {
                let models = try await Self.fetchOpenAICompatibleModels(apiKey:
                mistralModels = models.isEmpty ? Self.defaultMistralModels : models
                if mistralModel.isEmpty || !mistralModels.contains(where: { $0.id == mistralModel }) {
                    mistralModel = mistralModels.first?.id ?? "mistral-large-latest"
                }
            } catch {
                AuditLog.log(.api, "Failed to fetch Mistral models: \(error.localizedDescription)")
                mistralModels = Self.defaultMistralModels
            }
        }
    }

    func fetchCodestralModels() {
        isFetchingCodestralModels = true
        let key = codestralAPIKey
        Task {
            defer { isFetchingCodestralModels = false }
            guard !key.isEmpty else {
                codestralModels = Self.defaultCodestralModels
                return
            }
            do {
                // Codestral key works on codestral.mistral.ai/v1/models
                let allModels = try await Self.fetchOpenAICompatibleModels(apiKe
                // Filter out embed models — keep only chat/completion models
                let chatModels = allModels.filter { !$0.id.lowercased().contains("embed") }
                let models = chatModels.isEmpty ? allModels : chatModels
                codestralModels = models.isEmpty ? Self.defaultCodestralModels : models
                if codestralModel.isEmpty || !codestralModels.contains(where: { $0.id == codestralModel }) {
                    codestralModel = codestralModels.first?.id ?? "codestral-latest"
                }
            } catch {
                AuditLog.log(.api, "Failed to fetch Codestral models: \(error.localizedDescription)")
                codestralModels = Self.defaultCodestralModels
            }
        }
    }

    func fetchVibeModels() {
        isFetchingVibeModels = true
        let key = vibeAPIKey
        Task {
            defer { isFetchingVibeModels = false }
            guard !key.isEmpty else {
                vibeModels = Self.defaultVibeModels
                return
            }
            do {
                let allModels = try await Self.fetchOpenAICompatibleModels(apiKe
                // Vibe key only works with *-latest models, not dated versions
                let filtered = allModels.filter {
                    $0.id.lowercased().contains("devstral") && $0.id.contains("latest")
                }
                let models = filtered.isEmpty ? allModels : filtered
                vibeModels = models.isEmpty ? Self.defaultVibeModels : models
                if vibeModel.isEmpty || !vibeModels.contains(where: { $0.id == vibeModel }) {
                    vibeModel = vibeModels.first?.id ?? "devstral-latest"
                }
            } catch {
                AuditLog.log(.api, "Failed to fetch Vibe models: \(error.localizedDescription)")
                vibeModels = Self.defaultVibeModels
            }
        }
    }

    /// Shared OpenAI-compatible model list fetcher
    private nonisolated static func fetchOpenAICompatibleModels(apiKey: String, endpoint: String) async throws -> [OpenAIModelInfo] {
        guard let url = URL(string: endpoint) else { throw AgentError.invalidURL }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.timeoutInterval = llmAPITimeout
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200 else { return [] }
        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let modelsData = json["data"] as? [[String: Any]] else { return [] }
        return modelsData.compactMap { model -> OpenAIModelInfo? in
            guard let id = model["id"] as? String else { return nil }
            return OpenAIModelInfo(id: id, name: id)
        }.sorted { $0.name < $1.name }
    }

    // MARK: - vLLM Models

    func fetchVLLMModels() {
        isFetchingVLLMModels = true
        let endpoint = vLLMEndpoint
        let key = vLLMAPIKey
        Task {
            defer { isFetchingVLLMModels = false }
            do {
                let models = try await Self.fetchVLLMModelsFromAPI(endpoint: endpoint, apiKey: key)
                vLLMModels = models
                let ids = models.map(\.id)
                if vLLMModel.isEmpty || (!ids.isEmpty && !ids.contains(vLLMModel)) {
                    vLLMModel = ids.first ?? ""
                }
            } catch {
                appendLog("Failed to fetch vLLM models: \(error.localizedDescription)")
            }
        }
    }

    private nonisolated static func fetchVLLMModelsFromAPI(endpoint: String, apiKey: String) async throws -> [OpenAIModelInfo] {
        let modelsURL: URL
        if let range = endpoint.range(of: "/v1/") {
            let base = String(endpoint[endpoint.startIndex..<range.upperBound])
            guard let url = URL(string: base + "models") else { throw AgentError.invalidURL }
            modelsURL = url
        } else {
            guard let url = URL(string: endpoint) else { throw AgentError.invalidURL }
            modelsURL = url.deletingLastPathComponent().appendingPathComponent("models")
        }
        var request = URLRequest(url: modelsURL)
        request.httpMethod = "GET"
        if !apiKey.isEmpty { request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization") }
        request.timeoutInterval = llmAPITimeout
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200,
              let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let modelsData = json["data"] as? [[String: Any]] else { return [] }
        return modelsData.compactMap { model -> OpenAIModelInfo? in
            guard let id = model["id"] as? String else { return nil }
            return OpenAIModelInfo(id: id, name: id)
        }.sorted { $0.name < $1.name }
    }

    // MARK: - LM Studio Models

    func fetchLMStudioModels() {
        isFetchingLMStudioModels = true
        let proto = lmStudioProtocol
        let modelsEndpoint: String
        switch proto {
        case .lmStudio: modelsEndpoint = "http://localhost:1234/api/v1/models"
        default: modelsEndpoint = "http://localhost:1234/v1/models"
        }
        Task {
            defer { isFetchingLMStudioModels = false }
            do {
                let models = try await Self.fetchLMStudioModelsFromAPI(modelsURL: modelsEndpoint)
                lmStudioModels = models
                let ids = models.map(\.id)
                if lmStudioModel.isEmpty || (!ids.isEmpty && !ids.contains(lmStudioModel)) {
                    lmStudioModel = ids.first ?? ""
                }
            } catch {
                appendLog("Failed to fetch LM Studio models: \(error.localizedDescription)")
            }
        }
    }

    private nonisolated static func fetchLMStudioModelsFromAPI(modelsURL: String) async throws -> [OpenAIModelInfo] {
        guard let url = URL(string: modelsURL) else { throw AgentError.invalidURL }
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.timeoutInterval = llmAPITimeout
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200,
              let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let modelsData = json["data"] as? [[String: Any]] else { return [] }
        return modelsData.compactMap { model -> OpenAIModelInfo? in
            guard let id = model["id"] as? String else { return nil }
            return OpenAIModelInfo(id: id, name: id)
        }.sorted { $0.name < $1.name }
    }

    /// Trigger model fetch for a provider if its list is empty.
    func fetchModelsIfNeeded(for provider: APIProvider, force: Bool = false) {
        switch provider {
        case .claude: if force || availableClaudeModels.isEmpty { Task { await fetchClaudeModels() } }
        case .openAI: if force || openAIModels.isEmpty { fetchOpenAIModels() }
        case .ollama: if force || ollamaModels.isEmpty { fetchOllamaModels() }
        case .localOllama: if force || localOllamaModels.isEmpty { fetchLocalOllamaModels() }
        case .deepSeek: if force || deepSeekModels.isEmpty { fetchDeepSeekModels() }
        case .huggingFace: if force || huggingFaceModels.isEmpty { fetchHuggingFaceModels() }
        case .vLLM: if force || vLLMModels.isEmpty { fetchVLLMModels() }
        case .lmStudio: if force || lmStudioModels.isEmpty { fetchLMStudioModels() }
        case .zAI: if force || zAIModels.isEmpty { fetchZAIModels() }
        case .qwen: if force || qwenModels.isEmpty { fetchQwenModels() }
        case .gemini: if force || geminiModels.isEmpty { fetchGeminiModels() }
        case .grok: if force || grokModels.isEmpty { fetchGrokModels() }
        case .mistral: if force || mistralModels.isEmpty { fetchMistralModels() }
        case .codestral: if force || codestralModels.isEmpty { fetchCodestralModels() }
        case .vibe: if force || vibeModels.isEmpty { fetchVibeModels() }
        case .bigModel: break
        default: break
        }
    }
}
