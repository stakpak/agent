import SwiftUI
import AppKit

// MARK: - Plain Text Editor (NSTextView wrapper)

/// NSTextView subclass that inserts spaces instead of tabs.
private class PromptTextView: NSTextView {
    override func insertTab(_ sender: Any?) {
        insertText("    ", replacementRange: selectedRange())
    }
}

/// NSViewRepresentable wrapping NSTextView for plain-text editing with line numbers.
struct PromptEditor: NSViewRepresentable {
    @Binding var text: String
    var textColor: NSColor = .labelColor

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let textView = PromptTextView()
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.isContinuousSpellCheckingEnabled = false
        textView.isGrammarCheckingEnabled = false
        textView.isAutomaticLinkDetectionEnabled = false
        textView.isAutomaticDataDetectionEnabled = false
        textView.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        textView.textColor = textColor
        textView.insertionPointColor = textColor
        textView.backgroundColor = .textBackgroundColor
        textView.isEditable = true
        textView.isSelectable = true
        textView.allowsUndo = true
        textView.isRichText = false
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.textContainer?.widthTracksTextView = true
        textView.delegate = context.coordinator
        textView.string = text

        let scrollView = NSScrollView()
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = false
        scrollView.documentView = textView
        scrollView.drawsBackground = false

        if let ruler = PromptLineNumberRuler(textView: textView) {
            ruler.clipsToBounds = true
            scrollView.verticalRulerView = ruler
        }
        scrollView.hasVerticalRuler = true
        scrollView.rulersVisible = true

        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = scrollView.documentView as? NSTextView else { return }
        // Only update if the binding changed externally (not from user typing)
        if textView.string != text {
            textView.string = text
        }
    }

    class Coordinator: NSObject, NSTextViewDelegate {
        var parent: PromptEditor
        var pendingText = ""
        var pendingColor: NSColor = .labelColor
        weak var pendingScrollView: NSScrollView?
        init(_ parent: PromptEditor) { self.parent = parent }

        func textDidChange(_ notification: Notification) {
            guard let textView = notification.object as? NSTextView else { return }
            parent.text = textView.string
        }
    }
}

// MARK: - Line Number Ruler

private class PromptLineNumberRuler: NSRulerView {
    private var textView: NSTextView? { clientView as? NSTextView }
    private let gutterFont = NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
    private nonisolated(unsafe) var textObserver: NSObjectProtocol?

    init?(textView: NSTextView) {
        guard let scrollView = textView.enclosingScrollView else { return nil }
        super.init(scrollView: scrollView, orientation: .verticalRuler)
        self.clientView = textView
        ruleThickness = 36

        textObserver = NotificationCenter.default.addObserver(
            forName: NSText.didChangeNotification,
            object: textView, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.needsDisplay = true }
        }
    }

    required init(coder: NSCoder) { fatalError() }

    deinit {
        let obs = textObserver
        if let obs { NotificationCenter.default.removeObserver(obs) }
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let tv = textView, let lm = tv.layoutManager, let tc = tv.textContainer else { return }

        let isDark = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        let bgColor = isDark
            ? NSColor(red: 0.14, green: 0.14, blue: 0.16, alpha: 1.0)
            : NSColor(red: 0.95, green: 0.95, blue: 0.95, alpha: 1.0)
        let numColor = isDark
            ? NSColor(red: 0.45, green: 0.45, blue: 0.50, alpha: 1.0)
            : NSColor(red: 0.55, green: 0.55, blue: 0.58, alpha: 1.0)

        bgColor.setFill()
        rect.fill()

        let attrs: [NSAttributedString.Key: Any] = [.font: gutterFont, .foregroundColor: numColor]
        let text = tv.string as NSString
        guard let sv = scrollView else { return }
        let visibleRect = sv.contentView.bounds
        let textContainerInset = tv.textContainerInset

        guard text.length > 0 else {
            let numStr = "1" as NSString
            let strSize = numStr.size(withAttributes: attrs)
            let x = ruleThickness - strSize.width - 6
            let lineHeight = lm.defaultLineHeight(for: gutterFont)
            let y = textContainerInset.height - visibleRect.origin.y + (lineHeight - strSize.height) / 2.0 + 2.0
            numStr.draw(at: NSPoint(x: x, y: y), withAttributes: attrs)
            return
        }

        let visibleGlyphRange = lm.glyphRange(forBoundingRect: visibleRect, in: tc)
        let visibleCharRange = lm.characterRange(forGlyphRange: visibleGlyphRange, actualGlyphRange: nil)

        var lineNumber = 1
        var idx = 0
        while idx < visibleCharRange.location {
            let lineRange = text.lineRange(for: NSRange(location: idx, length: 0))
            lineNumber += 1
            idx = NSMaxRange(lineRange)
        }

        idx = visibleCharRange.location
        while idx <= NSMaxRange(visibleCharRange) {
            let lineRange = text.lineRange(for: NSRange(location: idx, length: 0))
            let glyphIdx = lm.glyphIndexForCharacter(at: idx)
            var lineRect = lm.lineFragmentRect(forGlyphAt: glyphIdx, effectiveRange: nil)
            lineRect.origin.y += textContainerInset.height
            lineRect.origin.y -= visibleRect.origin.y

            let numStr = "\(lineNumber)" as NSString
            let strSize = numStr.size(withAttributes: attrs)
            let x = ruleThickness - strSize.width - 6
            let y = lineRect.origin.y + (lineRect.height - strSize.height) / 2.0
            numStr.draw(at: NSPoint(x: x, y: y), withAttributes: attrs)

            lineNumber += 1
            idx = NSMaxRange(lineRange)
            if idx >= text.length { break }
        }
    }
}

// MARK: - System Prompt Window (standalone popup)

@MainActor
final class SystemPromptWindow {
    static let shared = SystemPromptWindow()
    private var window: NSWindow?

    func show() {
        if let w = window, w.isVisible {
            w.makeKeyAndOrderFront(nil)
            return
        }

        let view = SystemPromptsView()
        let hostingView = NSHostingView(rootView: view)
        hostingView.frame = NSRect(x: 0, y: 0, width: 700, height: 460)

        let w = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 700, height: 460),
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        w.title = "System Prompts"
        w.contentView = hostingView
        w.minSize = NSSize(width: 500, height: 350)
        w.center()
        w.isReleasedWhenClosed = false
        w.makeKeyAndOrderFront(nil)
        window = w
    }
}

// MARK: - System Prompts View (two tabs: System Prompt + Apple AI Compact)

struct SystemPromptsView: View {
    @State private var isCompact = false
    @State private var fullText = ""
    @State private var compactText = ""
    @State private var isFullDirty = false
    @State private var isCompactDirty = false

    private let service = SystemPromptService.shared

    private var currentText: Binding<String> {
        Binding(
            get: { isCompact ? compactText : fullText },
            set: {
                if isCompact { compactText = $0; isCompactDirty = true }
                else { fullText = $0; isFullDirty = true }
            }
        )
    }

    private var currentDirty: Bool { isCompact ? isCompactDirty : isFullDirty }

    private let fullColor = Color(red: 0.20, green: 0.90, blue: 0.30) // Terminal green
    private let fullNSColor = NSColor(red: 0.20, green: 0.90, blue: 0.30, alpha: 1.0)
    private let compactColor = Color(red: 1.0, green: 0.40, blue: 0.60) // Apple pink
    private let compactNSColor = NSColor(red: 1.0, green: 0.40, blue: 0.60, alpha: 1.0)

    var body: some View {
        VStack(spacing: 0) {
            // Tab bar
            HStack(spacing: 0) {
                Spacer().frame(width: 4)

                Button(action: { isCompact = false }) {
                    Text("System Prompt")
                        .font(.system(.caption, design: .monospaced))
                        .fontWeight(!isCompact ? .bold : .regular)
                        .fixedSize()
                        .padding(.horizontal, 8)
                        .padding(.vertical, 6)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .background(!isCompact ? fullColor.opacity(0.3) : Color.clear)
                .foregroundColor(!isCompact ? fullColor : .secondary)
                .cornerRadius(6)

                Button(action: { isCompact = true }) {
                    Text("Apple AI (Compact)")
                        .font(.system(.caption, design: .monospaced))
                        .fontWeight(isCompact ? .bold : .regular)
                        .fixedSize()
                        .padding(.horizontal, 8)
                        .padding(.vertical, 6)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .background(isCompact ? compactColor.opacity(0.3) : Color.clear)
                .foregroundColor(isCompact ? compactColor : .secondary)
                .cornerRadius(6)

                Spacer(minLength: 0)
            }
            .padding(.vertical, 4)
            .background(Color(nsColor: .controlBackgroundColor))

            Divider()

            // Editor
            PromptEditor(text: currentText, textColor: isCompact ? compactNSColor : fullNSColor)
                .id(isCompact)

            Divider()

            // Bottom bar
            HStack(spacing: 8) {
                Button("Reset to Default") {
                    service.resetToDefault(compact: isCompact)
                    if isCompact {
                        compactText = service.rawTemplate(compact: true)
                        isCompactDirty = false
                    } else {
                        fullText = service.rawTemplate(compact: false)
                        isFullDirty = false
                    }
                }
                .controlSize(.small)

                if currentDirty {
                    Text("Unsaved")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }

                Spacer()

                Button("Save") {
                    service.saveTemplate(isCompact ? compactText : fullText, compact: isCompact)
                    if isCompact { isCompactDirty = false }
                    else { isFullDirty = false }
                }
                .controlSize(.small)
                .keyboardShortcut("s", modifiers: .command)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 6)
            .background(Color(nsColor: .controlBackgroundColor).opacity(0.8))
        }
        .frame(minWidth: 500, minHeight: 350)
        .onAppear {
            service.ensureDefaults()
            fullText = service.rawTemplate(compact: false)
            compactText = service.rawTemplate(compact: true)
        }
    }
}
