import SwiftUI
import AppKit
import AgentColorSyntax
import AgentTerminalNeo

// MARK: - Coordinator: Render Pipeline

extension ActivityLogView.Coordinator {
    /// All rendering logic — runs on main thread but OUTSIDE SwiftUI's layout p
    func performRender() {
        guard let textView = latestTextView, let scrollView = latestScrollView else {

            return
        }
        let text = latestText
        let searchText = latestSearchText
        let caseSensitive = latestCaseSensitive
        let currentMatchIndex = latestMatchIndex
        let onMatchCount = latestMatchCallback
        let tabID = latestTabID

        if text.isEmpty {
            guard !showingPlaceholder else { return }
            textView.alphaValue = 0
            NSAnimationContext.runAnimationGroup { ctx in
                ctx.duration = 0.3
                textView.animator().alphaValue = 1
            }
            textView.textStorage?.setAttributedString(
                NSAttributedString(
                    string: "Ready. Enter a task below to begin.",
                    attributes: [.font: font, .foregroundColor: NSColor.secondaryLabelColor]
                )
            )
            showingPlaceholder = true
            lastLength = 0
            lastSearch = ""
            lastMatchIndex = -1
            clearCache()
            if let tabID { invalidateCache(for: tabID) }
            onMatchCount?(0)
            return
        }

        let len = (text as NSString).length
        let searchChanged = searchText != lastSearch || currentMatchIndex != lastMatchIndex
        let tabSwitched = forceTabSwitch || tabID != lastTabID
        forceTabSwitch = false

        let currentAppearance = scrollView.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua])
        let appearanceChanged = currentAppearance != lastAppearanceName
        if appearanceChanged {
            lastAppearanceName = currentAppearance
            lastLength = 0
            lastRenderedText = ""
            clearCache()
            invalidateAllCaches()
            CodeBlockTheme.updateAppearance()
            TerminalNeoTheme.updateAppearance()
        }

        guard len != lastLength || showingPlaceholder || searchChanged || tabSwitched || appearanceChanged else { return }

        let textChanged = len != lastLength || showingPlaceholder
        let textGrew = len > lastLength
        let searchCleared = searchText.isEmpty && !lastSearch.isEmpty
        showingPlaceholder = false

        if tabSwitched {
            if let storage = textView.textStorage, lastLength > 0, !lastRenderedText.isEmpty {
                cacheAttributedString(NSAttributedString(attributedString: storage), for: lastTabID, text: lastRenderedText)
            }
            lastTabID = tabID
            clearCache()
            // Reset lastLength to 0 so the textChanged path treats this as fres
            lastLength = 0
            lastRenderedText = ""
            userIsAtBottom = true
            // Fall through to textChanged path — same scroll behavior as first
        }

        if textChanged || searchCleared {
            // Source `activityLog` is bounded at 50K by `ScriptTab.trimLog`. Wh
            let prefixIntact: Bool = {
                guard lastLength > 0, !lastRenderedText.isEmpty else { return true }
                let n = min(64, min(text.count, lastRenderedText.count))
                guard n > 0 else { return true }
                return (text as NSString).substring(to: n) == (lastRenderedText as NSString).substring(to: n)
            }()
            let isAppending = len > lastLength && lastLength > 0 && !searchCleared && prefixIntact

            if isAppending {
                let prevLen = lastLength
                let newText = (text as NSString).substring(from: prevLen)
                // Auto-scroll to bottom when a new task starts
                if newText.contains(AgentViewModel.newTaskMarker) {
                    userIsAtBottom = true
                }
                let newLines = newText.components(separatedBy: "\n")
                let hasTableLines = newLines.contains { $0.trimmingCharacters(in: .whitespaces).hasPrefix("|") }
                let prevTail = (text as NSString).substring(to: prevLen).components(separatedBy: "\n").suffix(3)
                let prevHasTableLines = prevTail.contains { $0.trimmingCharacters(in: .whitespaces).hasPrefix("|") }

                if hasTableLines || prevHasTableLines {
                    let savedOrigin = scrollView.contentView.bounds.origin
                    let wasAtBottom = isNearBottom(textView)
                    textView.textStorage?.beginEditing()
                    textView.textStorage?.setAttributedString(buildAttributedString(from: text))
                    textView.textStorage?.endEditing()
                    lastLength = len
                    lastRenderedText = text
                    if !wasAtBottom {
                        scrollView.contentView.scroll(to: savedOrigin)
                        scrollView.reflectScrolledClipView(scrollView.contentView)
                    }
                } else {
                    // Freeze scroll position during text mutation to prevent te
                    let wasAtBottom = userIsAtBottom
                    let savedY = scrollView.contentView.bounds.origin.y

                    CATransaction.begin()
                    CATransaction.setDisableActions(true)
                    textView.textStorage?.beginEditing()
                    textView.textStorage?.append(renderMarkdownOnly(newText))
                    textView.textStorage?.endEditing()
                    CATransaction.commit()

                    // Restore scroll position if user was NOT at bottom
                    if !wasAtBottom {
                        isProgrammaticScroll = true
                        scrollView.contentView.scroll(to: NSPoint(x: 0, y: savedY))
                        scrollView.reflectScrolledClipView(scrollView.contentView)
                        isProgrammaticScroll = false
                    }

                    lastLength = len
                    lastRenderedText = text
                }
            } else {
                let savedOrigin = scrollView.contentView.bounds.origin
                let wasAtBottom = tabSwitched || isNearBottom(textView)
                // Try instant swap from cached TextStorage (no re-layout)
                if tabSwitched, swapToCachedStorage(for: tabID, text: text, textView: textView, scrollView: scrollView) {
                    // Cache hit — layout preserved, scroll restored
                } else {
                    textView.textStorage?.beginEditing()
                    textView.textStorage?.setAttributedString(buildAttributedString(from: text))
                    textView.textStorage?.endEditing()
                    if !wasAtBottom {
                        scrollView.contentView.scroll(to: savedOrigin)
                        scrollView.reflectScrolledClipView(scrollView.contentView)
                    }
                }
                lastLength = len
                lastRenderedText = text
            }
        }

        if !searchText.isEmpty || !lastSearch.isEmpty {
            if searchChanged {
                pendingRenderWork?.cancel()
                applySearchHighlighting(
                    textView: textView,
                    searchText: searchText,
                    caseSensitive: caseSensitive,
                    currentMatch: currentMatchIndex,
                    onMatchCount: onMatchCount
                )
            } else if textChanged && !searchText.isEmpty {
                pendingRenderWork?.cancel()
                let work = DispatchWorkItem { [weak self] in
                    guard let self, let tv = self.latestTextView else { return }
                    self.applySearchHighlighting(
                        textView: tv, searchText: self.latestSearchText,
                        caseSensitive: self.latestCaseSensitive,
                        currentMatch: self.latestMatchIndex,
                        onMatchCount: self.latestMatchCallback
                    )
                }
                pendingRenderWork = work
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: work)
            }
        }
        lastSearch = searchText
        lastMatchIndex = currentMatchIndex

        if textGrew {
            throttledScrollToEnd(textView)
        }
    }
}
