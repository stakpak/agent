import Foundation
import AppKit

extension WebAutomationService {
    // MARK: - Tab Switching

    /// Switch to a browser tab by index (0-based) or by title substring
    func switchTab(browser: String? = nil, index: Int? = nil, titleContains: String? = nil) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let script: String

        if let idx = index {
            switch browserId {
            case "com.apple.Safari":
                script = "tell application \"Safari\" to set current tab of front window to tab \(idx + 1) of front window"
            case "com.google.Chrome":
                script = "tell application \"Google Chrome\" to set active tab index of front window to \(idx + 1)"
            default:
                return "Error: tab switching not supported for this browser"
            }
        } else if let title = titleContains {
            let escaped = Self.escapeJS(title)
            switch browserId {
            case "com.apple.Safari":
                script = """
                tell application "Safari"
                    repeat with t in tabs of front window
                        if name of t contains "\(escaped)" then
                            set current tab of front window to t
                            return name of t
                        end if
                    end repeat
                    return "Tab not found"
                end tell
                """
            case "com.google.Chrome":
                script = """
                tell application "Google Chrome"
                    repeat with t in tabs of front window
                        if title of t contains "\(escaped)" then
                            set active tab index of front window to (index of t)
                            return title of t
                        end if
                    end repeat
                    return "Tab not found"
                end tell
                """
            default:
                return "Error: tab switching not supported for this browser"
            }
        } else {
            return "Error: specify index or titleContains"
        }

        let result = await Task.detached { () -> String in
            var err: NSDictionary?
            guard let appleScript = NSAppleScript(source: script) else { return "Error: script creation failed" }
            let out = appleScript.executeAndReturnError(&err)
            if let error = err { return "Error: \(error)" }
            return out.stringValue ?? "Switched tab"
        }.value
        return result
    }

    /// List open browser tabs
    func listTabs(browser: String? = nil) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let script: String

        switch browserId {
        case "com.apple.Safari":
            script = """
            tell application "Safari"
                set tabList to ""
                repeat with i from 1 to count of tabs of front window
                    set t to tab i of front window
                    set tabList to tabList & i & ". " & name of t & " — " & URL of t & linefeed
                end repeat
                return tabList
            end tell
            """
        case "com.google.Chrome":
            script = """
            tell application "Google Chrome"
                set tabList to ""
                repeat with i from 1 to count of tabs of front window
                    set t to tab i of front window
                    set tabList to tabList & i & ". " & title of t & " — " & URL of t & linefeed
                end repeat
                return tabList
            end tell
            """
        default:
            return "Error: tab listing not supported for this browser"
        }

        let result = await Task.detached { () -> String in
            var err: NSDictionary?
            guard let appleScript = NSAppleScript(source: script) else { return "Error: script creation failed" }
            let out = appleScript.executeAndReturnError(&err)
            if let error = err { return "Error: \(error)" }
            return out.stringValue ?? ""
        }.value
        return result
    }

    // MARK: - Window Management

    /// List all browser windows with their tabs
    func listWindows(browser: String? = nil) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let script: String

        switch browserId {
        case "com.apple.Safari":
            script = """
            tell application "Safari"
                set windowList to ""
                repeat with w from 1 to count of windows
                    set win to window w
                    set windowList to windowList & "Window " & w & ":"
                    if w = 1 then set windowList to windowList & " (front)"
                    set windowList to windowList & linefeed
                    repeat with t from 1 to count of tabs of win
                        set tabInfo to tab t of win
                        set windowList to windowList & "  " & t & ". " & name of tabInfo & " — " & URL of tabInfo & linefeed
                    end repeat
                end repeat
                return windowList
            end tell
            """
        case "com.google.Chrome":
            script = """
            tell application "Google Chrome"
                set windowList to ""
                repeat with w from 1 to count of windows
                    set win to window w
                    set windowList to windowList & "Window " & w & ":"
                    if w = 1 then set windowList to windowList & " (front)"
                    set windowList to windowList & linefeed
                    repeat with t from 1 to count of tabs of win
                        set tabInfo to tab t of win
                        set windowList to windowList & "  " & t & ". " & title of tabInfo & " — " & URL of tabInfo & linefeed
                    end repeat
                end repeat
                return windowList
            end tell
            """
        default:
            return "Error: window listing not supported for this browser"
        }

        return await runAppleScript(script)
    }

    /// Switch to a specific browser window by index (1-based)
    func switchWindow(browser: String? = nil, index: Int) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let script: String

        switch browserId {
        case "com.apple.Safari":
            script = """
            tell application "Safari"
                if \(index) > (count of windows) then return "Error: window \(index) does not exist"
                set index of window \(index) to 1
                return "Switched to window \(index): " & name of current tab of front window
            end tell
            """
        case "com.google.Chrome":
            script = """
            tell application "Google Chrome"
                if \(index) > (count of windows) then return "Error: window \(index) does not exist"
                set index of window \(index) to 1
                return "Switched to window \(index): " & title of active tab of front window
            end tell
            """
        default:
            return "Error: window switching not supported for this browser"
        }

        return await runAppleScript(script)
    }

    /// Open a new browser window
    func newWindow(browser: String? = nil, url: String? = nil) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let script: String

        switch browserId {
        case "com.apple.Safari":
            if let u = url {
                script = """
                tell application "Safari"
                    make new document with properties {URL:"\(Self.escapeJS(u))"}
                    return "New window opened: \(Self.escapeJS(u))"
                end tell
                """
            } else {
                script = """
                tell application "Safari"
                    make new document
                    return "New window opened"
                end tell
                """
            }
        case "com.google.Chrome":
            if let u = url {
                script = """
                tell application "Google Chrome"
                    set newWin to make new window
                    set URL of active tab of newWin to "\(Self.escapeJS(u))"
                    return "New window opened: \(Self.escapeJS(u))"
                end tell
                """
            } else {
                script = """
                tell application "Google Chrome"
                    make new window
                    return "New window opened"
                end tell
                """
            }
        default:
            return "Error: new window not supported for this browser"
        }

        return await runAppleScript(script)
    }

    /// Close a browser window by index (1-based). Defaults to front window.
    func closeWindow(browser: String? = nil, index: Int = 1) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let script: String

        switch browserId {
        case "com.apple.Safari":
            script = """
            tell application "Safari"
                if \(index) > (count of windows) then return "Error: window \(index) does not exist"
                close window \(index)
                return "Closed window \(index)"
            end tell
            """
        case "com.google.Chrome":
            script = """
            tell application "Google Chrome"
                if \(index) > (count of windows) then return "Error: window \(index) does not exist"
                close window \(index)
                return "Closed window \(index)"
            end tell
            """
        default:
            return "Error: close window not supported for this browser"
        }

        return await runAppleScript(script)
    }

    /// Shared AppleScript runner
    func runAppleScript(_ script: String) async -> String {
        await Task.detached { () -> String in
            var err: NSDictionary?
            guard let appleScript = NSAppleScript(source: script) else { return "Error: script creation failed" }
            let out = appleScript.executeAndReturnError(&err)
            if let error = err { return "Error: \(error)" }
            return out.stringValue ?? "OK"
        }.value
    }

    // MARK: - Wait for Element

    /// Wait for a CSS selector to appear in the page (polls via JavaScript)
    func waitForElement(selector: String, browser: String? = nil, timeout: TimeInterval = 10) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let escaped = Self.escapeJS(selector)
        let start = CFAbsoluteTimeGetCurrent()

        while CFAbsoluteTimeGetCurrent() - start < timeout {
            let js = "(function(){ var el = \(Self.querySelectorWithIframes("'\(escaped)'")); return el ? 'found' : 'waiting'; })()"
            if let result = try? await executeJavaScript(script: js, browser: browserId) as? String,
               result == "found"
            {
                return "Element found: \(selector)"
            }
            try? await Task.sleep(for: .milliseconds(500))
        }
        return "Timeout: element '\(selector)' not found after \(Int(timeout))s"
    }

    // MARK: - Scroll to Element

    /// Scroll until a CSS selector is visible, handling lazy-loaded content
    func scrollToElement(selector: String, browser: String? = nil, maxScrolls: Int = 20) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let escaped = Self.escapeJS(selector)
        let js = """
        (function() {
            var el = \(Self.querySelectorWithIframes("'\(escaped)'"));
            if (el) { el.scrollIntoView({behavior:'smooth',block:'center'}); return 'scrolled'; }
            return 'not found';
        })()
        """
        // First try — element might already exist
        if let result = try? await executeJavaScript(script: js, browser: browserId) as? String,
           result == "scrolled"
        {
            return "Scrolled to: \(selector)"
        }
        // Scroll down incrementally to trigger lazy loading
        for i in 0..<maxScrolls {
            _ = try? await executeJavaScript(script: "window.scrollBy(0, window.innerHeight * 0.8)", browser: browserId)
            try? await Task.sleep(for: .milliseconds(300))
            if let result = try? await executeJavaScript(script: js, browser: browserId) as? String,
               result == "scrolled"
            {
                return "Scrolled to: \(selector) (after \(i + 1) scroll(s))"
            }
        }
        return "Element '\(selector)' not found after scrolling \(maxScrolls) times"
    }

    // MARK: - Select Dropdown

    /// Select an option in a <select> dropdown by value, text, or index
    func selectOption(
        selector: String,
        value: String? = nil,
        text: String? = nil,
        index: Int? = nil,
        browser: String? = nil
    ) async -> String
    {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let escapedSel = Self.escapeJS(selector)

        let setOption: String
        if let val = value {
            setOption = "el.value = '\(Self.escapeJS(val))';"
        } else if let txt = text {
            let esc = Self.escapeJS(txt)
            setOption = "for(var i=0;i<el.options.length;i++){if(el.options[i].text.indexOf('\(esc)')>=0){el.selectedIndex=i;break;}}"
        } else if let idx = index {
            setOption = "el.selectedIndex = \(idx);"
        } else {
            return "Error: specify value, text, or index"
        }

        let js = """
        (function() {
            var el = \(Self.querySelectorWithIframes("'\(escapedSel)'"));
            if (!el || el.tagName !== 'SELECT') return 'not found or not a select';
            \(setOption)
            el.dispatchEvent(new Event('change', {bubbles: true}));
            return 'selected: ' + el.options[el.selectedIndex].text;
        })()
        """
        if let result = try? await executeJavaScript(script: js, browser: browserId) as? String {
            return result
        }
        return "Error: could not select option"
    }

    // MARK: - File Upload

    /// Trigger file upload dialog for an <input type="file"> via accessibility click
    /// Note: JS cannot set file input values (browser security). This clicks the input to open the file picker.
    func triggerFileUpload(selector: String, browser: String? = nil) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let escaped = Self.escapeJS(selector)
        let js = """
        (function() {
            var el = \(Self.querySelectorWithIframes("'\(escaped)'"));
            if (!el) return 'not found';
            el.click();
            return 'file dialog triggered';
        })()
        """
        if let result = try? await executeJavaScript(script: js, browser: browserId) as? String {
            return result
        }
        return "Error: could not trigger file upload"
    }

    // MARK: - Cookie / localStorage

    /// Read cookies or localStorage for the current page
    func readStorage(type: String = "cookies", key: String? = nil, browser: String? = nil) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let js: String
        switch type {
        case "localStorage":
            if let k = key {
                js = "localStorage.getItem('\(Self.escapeJS(k))') || '(not set)'"
            } else {
                js =
                    "(function(){var r={};for(var i=0;i<localStorage.length;i++){var k=localStorage.key(i);r[k]=localStorage.getItem(k);}return JSON.stringify(r);})()"
            }
        case "sessionStorage":
            if let k = key {
                js = "sessionStorage.getItem('\(Self.escapeJS(k))') || '(not set)'"
            } else {
                js =
                    "(function(){var r={};for(var i=0;i<sessionStorage.length;i++){var k=sessionStorage.key(i);r[k]=sessionStorage.getItem(k);}return JSON.stringify(r);})()"
            }
        default: // cookies
            js = "document.cookie || '(no cookies)'"
        }
        if let result = try? await executeJavaScript(script: js, browser: browserId) as? String {
            return result
        }
        return "Error: could not read \(type)"
    }

    // MARK: - Form Submit

    /// Submit a form by selector or find the closest form to an element
    func submitForm(selector: String? = nil, browser: String? = nil) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let js: String
        if let sel = selector {
            let escaped = Self.escapeJS(sel)
            js = """
            (function() {
                var el = \(Self.querySelectorWithIframes("'\(escaped)'"));
                if (!el) return 'not found';
                var form = el.tagName === 'FORM' ? el : el.closest('form');
                if (!form) return 'no form found';
                form.dispatchEvent(new Event('submit', {bubbles:true,cancelable:true}));
                form.submit();
                return 'submitted';
            })()
            """
        } else {
            js = "(function(){var f=document.querySelector('form');if(f){f.submit();return 'submitted';}return 'no form found';})()"
        }
        if let result = try? await executeJavaScript(script: js, browser: browserId) as? String {
            return result
        }
        return "Error: could not submit form"
    }

    // MARK: - Browser Navigation

    /// Navigate back, forward, or reload
    func navigate(action: String, browser: String? = nil) async -> String {
        let browserId = browser ?? detectActiveBrowser() ?? "com.apple.Safari"
        let js: String
        switch action {
        case "back": js = "history.back(); 'navigated back'"
        case "forward": js = "history.forward(); 'navigated forward'"
        case "reload": js = "location.reload(); 'reloaded'"
        default: return "Error: unknown action '\(action)'. Use back, forward, or reload."
        }
        _ = try? await executeJavaScript(script: js, browser: browserId)
        return "Navigated: \(action)"
    }

}
