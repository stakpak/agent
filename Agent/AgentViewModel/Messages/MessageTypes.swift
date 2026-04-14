//  AgentViewModel+MessageTypes.swift Agent  Extracted types for Messages m...

import Foundation

// MARK: - Message Filter Types

extension AgentViewModel {

    /// Filter for which messages to monitor
    enum MessageFilter: String, CaseIterable {
        case fromOthers = "From Others"
        case fromMe = "From Me"
        case noFilter = "Both"
    }

    /// Chat recipients discovered from Messages database
    struct MessageRecipient: Identifiable, Hashable {
        let id: String // handle id (phone/email) — used as stable key for filte
        let displayName: String
        let service: String // "iMessage" or "SMS"
        let fromMe: Bool // true if discovered from a sent message
    }

    /// Raw message data from chat.db
    struct RawMessage: Sendable {
        let rowid: Int
        let text: String
        let handleId: String
        let handleRowId: Int
        let chatId: Int
        let service: String
        let account: String
    }
}
