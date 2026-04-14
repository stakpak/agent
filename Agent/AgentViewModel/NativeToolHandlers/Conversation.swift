
@preconcurrency import Foundation
import AgentTools
import AgentMCP
import AgentD1F
import AgentSwift
import AgentAccess
import Cocoa

// MARK: - Native Tool Handler — Conversation Tools

extension AgentViewModel {

    /// / Handles write_text, transform_text, send_message, fix_text, plan_mode,
    func handleConversationNativeTool(name: String, input: [String: Any]) async -> String? {
        let pf = projectFolder
        switch name {
        // MARK: - Conversation Tools

        // write_text
        case "write_text":
            guard let subject = input["subject"] as? String, !subject.isEmpty else {
                return "Error: subject is required for write_text"
            }

            let style = input["style"] as? String ?? "informative"
            let lengthStr = input["length"] as? String ?? "medium"
            let context = input["context"] as? String ?? ""

            let targetWords: Int
            if let exactWords = Int(lengthStr) {
                targetWords = exactWords
            } else {
                switch lengthStr.lowercased() {
                case "short": targetWords = 100
                case "medium": targetWords = 300
                case "long": targetWords = 600
                default: targetWords = 300
                }
            }

            let guidance = """
            Generate \(style) text about "\(subject)" in approximately \(targetWords) words.
            Style: \(style)
            \(context.isEmpty ? "" : "Context: \(context)")
            Requirements: No emojis, well-structured paragraphs, clear and accurate.
            Begin your response directly with the text content.
            """

            return guidance
        // transform_text
        case "transform_text":
            guard let text = input["text"] as? String, !text.isEmpty else {
                return "Error: text is required for transform_text"
            }

            guard let transform = input["transform"] as? String, !transform.isEmpty else {
                return "Error: transform type is required for transform_text"
            }

            let options = input["options"] as? String ?? ""

            // Validate transform type
            let validTransforms = ["grocery_list", "todo_list", "outline", "summary", "bullet_points", "numbered_list", "table", "qa"]
            guard validTransforms.contains(transform.lowercased()) else {
                return "Error: invalid transform type. Valid types: \(validTransforms.joined(separator: ", "))"
            }

            let guidance: String

            switch transform.lowercased() {
            case "grocery_list":
                guidance = """
                Transform the following text into a grocery list format.

                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")

                Requirements:
                - Extract all items that could be grocery/shopping items
                - Format as a clean grocery list organized by category (produce, dairy, meat, pantry, etc.)
                - One item per line
                - No emojis - plain text only
                - Include quantities if mentioned

                Output the grocery list now:
                """

            case "todo_list":
                guidance = """
                Transform the following text into a todo/checklist format.

                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")

                Requirements:
                - Extract all actionable tasks
                - Format as a numbered or bulleted todo list
                - Each item should start with a verb (Buy, Call, Fix, etc.)
                - Group related tasks if possible
                - No emojis - plain text only

                Output the todo list now:
                """

            case "outline":
                guidance = """
                Transform the following text into a structured outline.

                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")

                Requirements:
                - Create hierarchical outline with main topics and subtopics
                - Use Roman numerals (I, II, III) for main sections
                - Use letters (A, B, C) for subsections
                - Use numbers (1, 2, 3) for details
                - No emojis - plain text only

                Output the outline now:
                """

            case "summary":
                guidance = """
                Summarize the following text concisely.

                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")

                Requirements:
                - Capture key points in brief
                - Keep summary to about 20% of original length
                - Maintain essential information
                - No emojis - plain text only

                Output the summary now:
                """

            case "bullet_points":
                guidance = """
                Transform the following text into bullet points.

                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")

                Requirements:
                - Extract key points as individual bullets
                - Use hyphens (-) for bullet points
                - Keep each point concise
                - No emojis - plain text only

                Output the bullet points now:
                """

            case "numbered_list":
                guidance = """
                Transform the following text into a numbered list.

                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")

                Requirements:
                - Extract items as a numbered sequence
                - Use 1., 2., 3. format
                - Maintain logical order
                - No emojis - plain text only

                Output the numbered list now:
                """

            case "table":
                guidance = """
                Transform the following text into a table format.

                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")

                Requirements:
                - Organize information into columns
                - Use pipe (|) separators for table format
                - Include header row
                - No emojis - plain text only

                Output the table now:
                """

            case "qa":
                guidance = """
                Transform the following text into Q&A format.

                Original text:
                \(text)
                \(options.isEmpty ? "" : "Options: \(options)")

                Requirements:
                - Generate relevant questions from the content
                - Provide clear answers
                - Format as Q: question, A: answer pairs
                - No emojis - plain text only

                Output the Q&A now:
                """

            default:
                guidance = "Transform this text: \(text)"
            }

            return guidance
        // send_message
        case "send_message":
            guard let content = input["content"] as? String, !content.isEmpty else {
                return "Error: content is required for send_message"
            }

            guard let recipient = input["recipient"] as? String, !recipient.isEmpty else {
                return "Error: recipient is required for send_message"
            }

            let channel = input["channel"] as? String ?? "imessage"
            let subject = input["subject"] as? String ?? ""

            // Ensure no emojis in content (simple emoji removal)
            let cleanContent = content.unicodeScalars.filter { !isEmoji($0) }.map(String.init).joined()

            // Handle different channels
            switch channel.lowercased() {
            case "clipboard":
                // Copy to clipboard
                await MainActor.run {
                    let pasteboard = NSPasteboard.general
                    pasteboard.clearContents()
                    pasteboard.setString(cleanContent, forType: .string)
                }
                return "Message copied to clipboard:\n\(cleanContent)"

            case "imessage":
                // Use AppleScript to send iMessage (simplified version)
                let escapedRecipient = recipient.replacingOccurrences(of: "\"", with: "\\\"")
                let escapedContent = cleanContent.replacingOccurrences(of: "\"", with: "\\\"")

                let script = """
                tell application "Messages"
                    send "\(escapedContent)" to buddy "\(escapedRecipient)"
                end tell
                """

                let result = await Self.offMain { () -> String in
                    var err: NSDictionary?
                    guard let applescript = NSAppleScript(source: script) else {
                        return "Error: Failed to create AppleScript"
                    }
                    let _ = applescript.executeAndReturnError(&err)
                    if let e = err {
                        return "AppleScript error: \(e)"
                    }
                    return "iMessage sent to \(recipient)"
                }
                return result

            case "email":
                // Open mailto URL
                let escapedSubject = subject.addingPercentEncoding(withAllowedCharacters: CharacterSet.urlQueryAllowed) ?? ""
                let escapedBody = cleanContent.addingPercentEncoding(withAllowedCharacters: CharacterSet.urlQueryAllowed) ?? ""
                let mailtoURL: String

                if recipient.lowercased() == "me" {
                    mailtoURL = "mailto:?subject=\(escapedSubject)&body=\(escapedBody)"
                } else {
                    let escapedRecipient = recipient.addingPercentEncoding(withAllowedCharacters: CharacterSet.urlQueryAllowed) ?? recipient
                    mailtoURL = "mailto:\(escapedRecipient)?subject=\(escapedSubject)&body=\(escapedBody)"
                }

                await MainActor.run {
                    if let url = URL(string: mailtoURL) {
                        NSWorkspace.shared.open(url)
                    }
                }
                return "Email draft opened for \(recipient)"

            case "sms":
                // Open SMS URL scheme
                let escapedBody = cleanContent.addingPercentEncoding(withAllowedCharacters: CharacterSet.urlQueryAllowed) ?? ""
                let smsURL = "sms:\(recipient)?body=\(escapedBody)"

                await MainActor.run {
                    if let url = URL(string: smsURL) {
                        NSWorkspace.shared.open(url)
                    }
                }
                return "SMS draft opened for \(recipient)"

            default:
                return "Error: Unsupported channel '\(channel)'. Use: imessage, email, sms, or clipboard"
            }
        // fix_text
        case "fix_text":
            guard let text = input["text"] as? String, !text.isEmpty else {
                return "Error: text is required for fix_text"
            }

            let fixes = input["fixes"] as? String ?? "all"
            let preserveStyle = input["preserve_style"] as? Bool ?? true

            // Validate fixes type
            let validFixes = ["all", "spelling", "grammar", "punctuation", "capitalization"]
            guard validFixes.contains(fixes.lowercased()) else {
                return "Error: invalid fixes type. Valid types: \(validFixes.joined(separator: ", "))"
            }

            let guidance: String

            switch fixes.lowercased() {
            case "spelling":
                guidance = """
                Fix spelling errors in the following text.

                Original text:
                \(text)

                Requirements:
                - Correct all spelling mistakes
                - Preserve original meaning and style: \(preserveStyle ? "yes" : "no")
                - Do NOT add any emojis
                - Do NOT change word choices unless misspelled
                - Return only the corrected text

                Corrected text:
                """

            case "grammar":
                guidance = """
                Fix grammar errors in the following text.

                Original text:
                \(text)

                Requirements:
                - Correct grammar, verb tense, and sentence structure
                - Preserve original meaning and style: \(preserveStyle ? "yes" : "no")
                - Do NOT add any emojis
                - Do NOT change wording unless grammatically incorrect
                - Return only the corrected text

                Corrected text:
                """

            case "punctuation":
                guidance = """
                Fix punctuation in the following text.

                Original text:
                \(text)

                Requirements:
                - Correct all punctuation errors
                - Fix spacing around punctuation
                - Preserve original meaning and style: \(preserveStyle ? "yes" : "no")
                - Do NOT add any emojis
                - Return only the corrected text

                Corrected text:
                """

            case "capitalization":
                guidance = """
                Fix capitalization in the following text.

                Original text:
                \(text)

                Requirements:
                - Correct capitalization (sentences start with capitals, proper nouns, etc.)
                - Preserve original meaning and style: \(preserveStyle ? "yes" : "no")
                - Do NOT add any emojis
                - Return only the corrected text

                Corrected text:
                """

            default: // "all"
                guidance = """
                Fix all spelling and grammar errors in the following text.

                Original text:
                \(text)

                Requirements:
                - Correct spelling, grammar, punctuation, and capitalization
                - Preserve original meaning and style: \(preserveStyle ? "yes" : "no")
                - Do NOT add any emojis
                - Keep the same tone and voice
                - Return only the corrected text

                Corrected text:
                """
            }

            return guidance
        // plan_mode
        case "plan_mode":
            let action: String = input["action"] as? String ?? "read"
            return Self.handlePlanMode(action: action, input: input, projectFolder: pf, tabName: "main")
        // project_folder
        case "project_folder":
            return handleProjectFolder(tab: nil, input: input)
        default:
            return nil
        }
    }
}
