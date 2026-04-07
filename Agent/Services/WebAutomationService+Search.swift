import Foundation
import AppKit

extension WebAutomationService {
    // MARK: - Safari Google Search

    /// Perform a Google search in Safari and return the results page content.
    /// Opens google.com, types the query, submits, waits for results, returns text.
    func safariGoogleSearch(query: String, maxResults: Int = 3000) async -> String {
        // 1. Open google.com
        let openScript = """
        tell application "Safari"
            activate
            if (count of windows) = 0 then make new document
            set URL of front document to "https://www.google.com"
        end tell
        """
        let openOK = await runAppleScript(openScript)
        guard !openOK.hasPrefix("Error") else { return openOK }

        // 2. Wait for search field to appear (not just readyState)
        for _ in 0..<20 {
            try? await Task.sleep(for: .milliseconds(500))
            let found = await runAppleScript("""
            tell application "Safari" to do JavaScript "document.querySelector('textarea[name=q],input[name=q]') ? 'ready' : 'waiting'" in front document
            """)
            if found == "ready" { break }
        }

        // 3. Type query and submit — use AppleScript string quoting (backslash-escape double quotes)
        let safeQuery = query.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
        let submitResult = await runAppleScript("""
        tell application "Safari" to do JavaScript "var el=document.querySelector('textarea[name=q],input[name=q]');if(el){el.focus();el.value=\\"\(safeQuery)\\";el.dispatchEvent(new Event('input',{bubbles:true}));var f=el.closest('form');if(f){f.submit();'submitted'}else{'no form'}}else{'not found'}" in front document
        """)
        guard submitResult == "submitted" else {
            return "{\"success\": false, \"error\": \"Search submit failed: \(submitResult)\"}"
        }

        // 4. Wait for results page to load
        for _ in 0..<20 {
            try? await Task.sleep(for: .milliseconds(300))
            let title = await runAppleScript("""
            tell application "Safari" to return name of front document
            """)
            if title.contains("Google Search") || title.contains("- Google") { break }
        }

        // Small extra wait for content to render
        try? await Task.sleep(for: .milliseconds(500))

        // 5. Get results
        let url = await runAppleScript("""
        tell application "Safari" to return URL of front document
        """)
        let title = await runAppleScript("""
        tell application "Safari" to return name of front document
        """)
        let content = await runAppleScript("""
        tell application "Safari" to do JavaScript "document.body.innerText.substring(0, \(maxResults))" in front document
        """)

        return """
        {"success": true, "query": "\(Self.escapeJS(query))", "url": "\(Self.escapeJS(url))", "title": "\(Self.escapeJS(title))", "content": "\(Self.escapeJS(content))"}
        """
    }

    /// Scan the current page for interactive elements (inputs, buttons, links, selects).
    func scanInteractiveElements(maxElements: Int = 50) async -> String {
        let js = """
        (function() {
            var results = [];
            var seen = new Set();
            function bestSelector(el) {
                if (el.id) return '#' + el.id;
                if (el.name) return el.tagName.toLowerCase() + '[name=' + JSON.stringify(el.name) + ']';
                var attr = el.getAttribute('aria-label');
                if (attr) return el.tagName.toLowerCase() + '[aria-label=' + JSON.stringify(attr) + ']';
                attr = el.getAttribute('placeholder');
                if (attr) return el.tagName.toLowerCase() + '[placeholder=' + JSON.stringify(attr) + ']';
                attr = el.getAttribute('type');
                if (attr && attr !== 'text') return el.tagName.toLowerCase() + '[type=' + JSON.stringify(attr) + ']';
                var role = el.getAttribute('role');
                if (role) return el.tagName.toLowerCase() + '[role=' + JSON.stringify(role) + ']';
                var parent = el.parentElement;
                if (parent) {
                    var siblings = parent.querySelectorAll(el.tagName);
                    if (siblings.length === 1) return bestSelector(parent) + ' > ' + el.tagName.toLowerCase();
                    for (var i = 0; i < siblings.length; i++) {
                        if (siblings[i] === el) return bestSelector(parent) + ' > ' + el.tagName.toLowerCase() + ':nth-child(' + (i+1) + ')';
                    }
                }
                return el.tagName.toLowerCase();
            }
            var selectors = [
                {sel: 'input:not([type=hidden])', type: 'input'},
                {sel: 'textarea', type: 'textarea'},
                {sel: 'button', type: 'button'},
                {sel: 'select', type: 'select'},
                {sel: '[role=button]', type: 'role-button'},
                {sel: '[role=search] input', type: 'search-input'},
                {sel: '[role=searchbox]', type: 'searchbox'},
                {sel: '[contenteditable=true]', type: 'editable'},
                {sel: 'a[href]', type: 'link'}
            ];
            selectors.forEach(function(s) {
                document.querySelectorAll(s.sel).forEach(function(el) {
                    if (results.length >= \(maxElements)) return;
                    var r = el.getBoundingClientRect();
                    if (r.width === 0 && r.height === 0) return;
                    var sel = bestSelector(el);
                    if (seen.has(sel)) return;
                    seen.add(sel);
                    var info = {
                        type: s.type,
                        tag: el.tagName.toLowerCase(),
                        selector: sel,
                        placeholder: el.placeholder || '',
                        ariaLabel: el.getAttribute('aria-label') || '',
                        text: (el.textContent || '').trim().substring(0, 60),
                        value: (el.value || '').substring(0, 60),
                        inputType: el.type || '',
                        href: (s.type === 'link' && el.href) ? el.href.substring(0, 120) : ''
                    };
                    results.push(info);
                });
            });
            return JSON.stringify(results);
        })()
        """.replacingOccurrences(of: "\n", with: " ")
        return await runAppleScript("""
        tell application "Safari" to do JavaScript "\(js.replacingOccurrences(of: "\"", with: "\\\""))" in front document
        """)
    }

    /// Search on the current page by finding a search input field, typing the query, and submitting.
    func safariSiteSearch(query: String) async -> String {
        // Try common search input selectors
        let escapedQuery = query.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "'", with: "\\'")
        let js = """
        (function() {
            var selectors = [
                'input[role=searchbox]', 'input[role=search]',
                '[role=search] input[type=text]', '[role=search] input:not([type=hidden])',
                'input[type=search]', 'input[name=q]', 'input[name=query]',
                'input[name=search]', 'input[name=keywords]',
                'input[aria-label*=earch]', 'input[aria-label*=Search]',
                'input[placeholder*=earch]', 'input[placeholder*=Search]',
                'input[placeholder*=looking]',
                'input[id*=search]', 'input[class*=search]',
                'textarea[role=searchbox]'
            ];
            for (var i = 0; i < selectors.length; i++) {
                var el = document.querySelector(selectors[i]);
                if (el && el.offsetWidth > 0) {
                    el.focus();
                    el.value = '\(escapedQuery)';
                    el.dispatchEvent(new Event('input', {bubbles: true}));
                    el.dispatchEvent(new Event('change', {bubbles: true}));
                    var ke = new KeyboardEvent('keydown', {key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true});
                    el.dispatchEvent(ke);
                    var ke2 = new KeyboardEvent('keypress', {key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true});
                    el.dispatchEvent(ke2);
                    var ke3 = new KeyboardEvent('keyup', {key: 'Enter', code: 'Enter', keyCode: 13, bubbles: true});
                    el.dispatchEvent(ke3);
                    var form = el.closest('form');
                    if (form) { form.submit(); }
                    return JSON.stringify({success: true, selector: selectors[i], field: el.placeholder || el.name || el.id});
                }
            }
            return JSON.stringify({success: false, error: 'No search field found on page'});
        })()
        """.replacingOccurrences(of: "\n", with: " ")
        let result = await runAppleScript("""
        tell application "Safari" to do JavaScript "\(js.replacingOccurrences(of: "\"", with: "\\\""))" in front document
        """)

        // Wait for results page to load
        try? await Task.sleep(for: .seconds(2))

        let url = await getPageURL()
        let title = await getPageTitle()
        let content = await readPageContent(maxLength: 3000)

        return """
        {"search": \(result), "url": "\(Self.escapeJS(url))", "title": "\(Self.escapeJS(title))", "content": "\(Self.escapeJS(content))"}
        """
    }
}
