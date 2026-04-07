import Foundation
import AVFoundation
import AgentAudit

@MainActor
final class SoundPlayer {
    static let shared = SoundPlayer()
    private var audioPlayer: AVAudioPlayer?

    func playStartupSound() {
        guard let soundURL = Bundle.main.url(
            forResource: "StartupTwentiethAnniversaryMac",
            withExtension: "wav") else { return }

        do {
            audioPlayer = try AVAudioPlayer(contentsOf: soundURL)
            audioPlayer?.prepareToPlay()
            audioPlayer?.volume = 0.75
            audioPlayer?.play()
        } catch {
            AuditLog.log(.shell, "Failed to load startup sound: \(error)")
        }
    }
}
