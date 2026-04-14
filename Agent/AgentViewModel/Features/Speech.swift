import Foundation
@preconcurrency import Speech
import AVFoundation

// MARK: - Speech-to-Text Dictation

extension AgentViewModel {

    func toggleDictation() {
        if isListening {
            stopDictation()
        } else {
            startDictation()
        }
    }

    func startDictation() {
        SFSpeechRecognizer.requestAuthorization { @Sendable status in
            Task { @MainActor [weak self] in
                guard let self else { return }
                switch status {
                case .authorized:
                    self.beginAudioSession()
                case .denied, .restricted:
                    self.appendLog("⚠️ Speech recognition not authorized. Enable in System Settings > Privacy > Speech Recognition.")
                case .notDetermined:
                    self.appendLog("⚠️ Speech recognition authorization not determined.")
                @unknown default:
                    break
                }
            }
        }
    }

    func stopDictation() {
        hotwordSilenceTimer?.invalidate()
        hotwordSilenceTimer = nil
        isHotwordCapturing = false
        hotwordLastTranscriptionLength = 0
        speechAudioEngine?.stop()
        speechAudioEngine?.inputNode.removeTap(onBus: 0)
        speechRecognitionRequest?.endAudio()
        speechRecognitionTask?.cancel()
        speechRecognitionTask = nil
        speechRecognitionRequest = nil
        speechAudioEngine = nil
        isListening = false
        preDictationTabId = nil
    }

    // MARK: - Hotword Listening

    func toggleHotwordListening() {
        if isHotwordListening {
            stopHotwordListening()
        } else {
            startHotwordListening()
        }
    }

    func startHotwordListening() {
        isHotwordListening = true
        isHotwordCapturing = false
        startDictation()
    }

    func stopHotwordListening() {
        isHotwordListening = false
        isHotwordCapturing = false
        stopDictation()
    }

    // MARK: - Private

    private func beginAudioSession() {
        stopDictation()

        guard let recognizer = SFSpeechRecognizer(), recognizer.isAvailable else {
            appendLog("⚠️ Speech recognizer not available for current locale.")
            return
        }

        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.addsPunctuation = true

        let engine = AVAudioEngine()
        let inputNode = engine.inputNode
        let recordingFormat = inputNode.outputFormat(forBus: 0)

        inputNode.installTap(onBus: 0, bufferSize: 1024, format: recordingFormat) { @Sendable buffer, _ in
            request.append(buffer)
        }

        engine.prepare()

        do {
            try engine.start()
        } catch {
            appendLog("❌ Audio engine failed: \(error.localizedDescription)")
            return
        }

        speechAudioEngine = engine
        speechRecognitionRequest = request
        isListening = true

        preDictationTabId = selectedTabId
        if let tabId = selectedTabId,
           let tab = tab(for: tabId)
        {
            preDictationText = tab.taskInput
        } else {
            preDictationText = taskInput
        }

        speechRecognitionTask = recognizer.recognitionTask(with: request) { @Sendable result, error in
            let transcription = result?.bestTranscription.formattedString
            let isFinal = result?.isFinal ?? false
            let hasError = error != nil
            Task { @MainActor [weak self] in
                guard let self, self.isListening else { return }

                if let transcription {
                    if self.isHotwordListening {
                        self.handleHotwordTranscription(transcription)
                    } else {
                        // Normal dictation mode
                        let prefix = self.preDictationText
                        let separator = prefix.isEmpty || prefix.hasSuffix(" ") ? "" : " "
                        let newText = prefix + separator + transcription

                        if let tabId = self.preDictationTabId,
                           let tab = self.tab(for: tabId)
                        {
                            tab.taskInput = newText
                        } else {
                            self.taskInput = newText
                        }
                    }
                }

                if hasError || isFinal {
                    if self.isHotwordListening {
                        // Restart listening after a pause (recognition sessions
                        self.restartHotwordSession()
                    } else {
                        self.stopDictation()
                    }
                }
            }
        }
    }

    // MARK: - Hotword Processing

    /// / Find the LAST word-boundary occurrence of "agent"/"agent!" in lowercas
    private static func wakeWordAnchor(in transcription: String) -> String.Index? {
        let lower = transcription.lowercased()
        let wakes = ["agent!", "agent"] // try the punctuated form first
        var bestEnd: String.Index?
        for wake in wakes {
            var searchStart = lower.startIndex
            while let range = lower.range(of: wake, range: searchStart..<lower.endIndex) {
                let beforeOK: Bool = {
                    guard range.lowerBound > lower.startIndex else { return true }
                    let prev = lower[lower.index(before: range.lowerBound)]
                    return !prev.isLetter
                }()
                let afterOK: Bool = {
                    guard range.upperBound < lower.endIndex else { return true }
                    let next = lower[range.upperBound]
                    return !next.isLetter
                }()
                if beforeOK && afterOK {
                    bestEnd = range.upperBound // keep walking — we want the LAS
                }
                searchStart = lower.index(after: range.lowerBound)
            }
            if bestEnd != nil { break } // prefer "agent!" over "agent" if both
        }
        return bestEnd
    }

    private func handleHotwordTranscription(_ transcription: String) {
        if !isHotwordCapturing {
            // Look for the wake word "agent" / "agent!" — must be a complete wo
            guard let anchor = Self.wakeWordAnchor(in: transcription) else { return }

            // Wake word detected — start capturing the command after it
            isHotwordCapturing = true
            let afterAgent = String(transcription[anchor...])
                .trimmingCharacters(in: CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: "!.,")))

            let command = afterAgent.isEmpty ? "" : afterAgent
            setInputText(command)
            hotwordLastTranscriptionLength = command.count
            resetSilenceTimer()
            return
        }

        // Already capturing
        if let anchor = Self.wakeWordAnchor(in: transcription) {
            let afterAgent = String(transcription[anchor...])
                .trimmingCharacters(in: CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: "!.,")))
            setInputText(afterAgent)

            if afterAgent.count != hotwordLastTranscriptionLength {
                hotwordLastTranscriptionLength = afterAgent.count
                resetSilenceTimer()
            }
        }
    }

    private func setInputText(_ text: String) {
        let prefix = preDictationText
        let separator = (prefix.isEmpty || prefix.hasSuffix(" ") || text.isEmpty) ? "" : " "
        let newText = prefix + separator + text

        if let tabId = preDictationTabId,
           let tab = tab(for: tabId)
        {
            tab.taskInput = newText
        } else {
            taskInput = newText
        }
    }

    private func resetSilenceTimer() {
        hotwordSilenceTimer?.invalidate()
        hotwordSilenceTimer = Timer.scheduledTimer(withTimeInterval: 2.5, repeats: false) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.submitHotwordCommand()
            }
        }
    }

    private func submitHotwordCommand() {
        hotwordSilenceTimer?.invalidate()
        hotwordSilenceTimer = nil

        // Stop current recognition
        speechAudioEngine?.stop()
        speechAudioEngine?.inputNode.removeTap(onBus: 0)
        speechRecognitionRequest?.endAudio()
        speechRecognitionTask?.cancel()
        speechRecognitionTask = nil
        speechRecognitionRequest = nil
        speechAudioEngine = nil
        isListening = false
        isHotwordCapturing = false

        // Submit the command
        if let tabId = preDictationTabId,
           let tab = tab(for: tabId)
        {
            if !tab.taskInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                runTabTask(tab: tab)
            }
        } else {
            if !taskInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                run()
            }
        }

        // Restart hotword listening after a short delay
        if isHotwordListening {
            Task { @MainActor [weak self] in
                try? await Task.sleep(for: .seconds(1))
                guard let self, self.isHotwordListening else { return }
                self.startDictation()
            }
        }
    }

    private func restartHotwordSession() {
        // Recognition timed out — restart if still in hotword mode
        speechAudioEngine?.stop()
        speechAudioEngine?.inputNode.removeTap(onBus: 0)
        speechRecognitionRequest?.endAudio()
        speechRecognitionTask?.cancel()
        speechRecognitionTask = nil
        speechRecognitionRequest = nil
        speechAudioEngine = nil
        isListening = false
        isHotwordCapturing = false
        hotwordLastTranscriptionLength = 0
        hotwordSilenceTimer?.invalidate()
        hotwordSilenceTimer = nil

        Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(0.5))
            guard let self, self.isHotwordListening else { return }
            self.startDictation()
        }
    }
}
