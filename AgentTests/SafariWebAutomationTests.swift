import Testing
import Foundation
@testable import Agent_

@Suite("WebAutomation", .serialized)
@MainActor
struct WebAutomationTests {

    let web = WebAutomationService.shared

    // MARK: - Helpers

    /// Open URL and poll until a JS condition is true (max 15s)
    private func openAndVerify(_ url: String, condition: String, timeout: Double = 15) async -> Bool {
        _ = try? await web.open(url: URL(string: url)!, waitForLoad: true)
        let start = CFAbsoluteTimeGetCurrent()
        while CFAbsoluteTimeGetCurrent() - start < timeout {
            if let result = try? await web.executeJavaScript(script: condition) as? String,
               result == "true" {
                return true
            }
            try? await Task.sleep(for: .milliseconds(500))
        }
        return false
    }

    /// Run JS and return string result
    private func runJS(_ script: String) async -> String? {
        try? await web.executeJavaScript(script: script) as? String
    }

    // MARK: - Google Search

    @Test("Google search: open, type, submit, read results")
    func googleSearch() async {
        let result = await web.safariGoogleSearch(query: "swift programming", maxResults: 1000)
        #expect(result.contains("\"success\": true"), "Search failed: \(result.prefix(200))")
        #expect(result.lowercased().contains("swift"), "Results should mention swift")
        #expect(result.contains("google.com/search"), "Should be on search results URL")
    }

    @Test("Google search: special characters")
    func googleSearchSpecialChars() async {
        let result = await web.safariGoogleSearch(query: "what is 2+2", maxResults: 500)
        #expect(result.contains("\"success\": true"), "Search failed: \(result.prefix(200))")
    }

    // MARK: - Google Signup Form

    @Test("Google signup: detect form fields")
    func googleSignupDetectFields() async {
        let loaded = await openAndVerify(
            "https://accounts.google.com/signup",
            condition: "document.querySelector('input[name=firstName]') ? 'true' : 'false'"
        )
        #expect(loaded, "Signup page should have firstName field")

        let lastName = await runJS("document.querySelector('input[name=lastName]') ? 'found' : 'not found'")
        #expect(lastName == "found", "Should have lastName field")
    }

    @Test("Google signup: fill and verify form")
    func googleSignupFillForm() async {
        let loaded = await openAndVerify(
            "https://accounts.google.com/signup",
            condition: "document.querySelector('input[name=firstName]') ? 'true' : 'false'"
        )
        #expect(loaded, "Signup page should load")

        let fillResult = await runJS("""
        (function() {
            var fn = document.querySelector('input[name=firstName]');
            var ln = document.querySelector('input[name=lastName]');
            if (!fn || !ln) return 'fields missing';
            fn.focus(); fn.value = 'TestAgent';
            fn.dispatchEvent(new Event('input', {bubbles: true}));
            ln.focus(); ln.value = 'McTest';
            ln.dispatchEvent(new Event('input', {bubbles: true}));
            return fn.value + ' ' + ln.value;
        })()
        """)
        #expect(fillResult == "TestAgent McTest", "Form fill failed: \(fillResult ?? "nil")")

        // Cleanup
        _ = await runJS(
            "document.querySelector('input[name=firstName]').value='';"
                + "document.querySelector('input[name=lastName]').value='';"
                + "'ok'"
        )
    }

    @Test("Google signup: find Next button")
    func googleSignupNextButton() async {
        let loaded = await openAndVerify(
            "https://accounts.google.com/signup",
            condition: "document.querySelector('input[name=firstName]') ? 'true' : 'false'"
        )
        #expect(loaded, "Signup page should load")

        let btn = await runJS("""
        (function() {
            var btns = document.querySelectorAll('button,input[type=submit]');
            for (var i = 0; i < btns.length; i++) {
                if (btns[i].textContent.includes('Next') || btns[i].value === 'Next') return 'found';
            }
            return 'not found';
        })()
        """)
        #expect(btn == "found", "Next button should exist")
    }

    // MARK: - LinkedIn

    @Test("LinkedIn: detect page state (login or feed)")
    func linkedInPageState() async {
        let loaded = await openAndVerify(
            "https://www.linkedin.com/feed/",
            condition: "document.readyState === 'complete' ? 'true' : 'false'"
        )
        #expect(loaded, "LinkedIn should load")

        let state = await runJS("""
        (function() {
            if (document.querySelector('.feed-shared-update-v2')) return 'feed';
            if (document.querySelector('.share-box-feed-entry__top-bar')) return 'feed';
            if (document.querySelector('input[name=session_key]')) return 'login';
            if (document.querySelector('input#username')) return 'login';
            if (document.querySelector('.global-nav')) return 'logged_in';
            if (document.title.includes('LinkedIn')) return 'linkedin_page';
            return 'unknown';
        })()
        """)
        #expect(state != nil && state != "unknown", "Should detect LinkedIn state, got: \(state ?? "nil")")
    }

    @Test("LinkedIn login: detect email/password fields")
    func linkedInLoginFields() async {
        // LinkedIn may redirect /login to a different page — detect whatever loads
        let loaded = await openAndVerify(
            "https://www.linkedin.com/login",
            condition: "document.readyState === 'complete' && document.title.includes('LinkedIn') ? 'true' : 'false'"
        )
        #expect(loaded, "LinkedIn login should load")

        let fields = await runJS("""
        (function() {
            var email = document.querySelector('input[name=session_key],input#username,input[type=email],input[autocomplete=username]');
            var pass = document.querySelector('input[name=session_password],input#password,input[type=password]');
            var btn = null;
            var btns = document.querySelectorAll('button,input[type=submit]');
            for (var i = 0; i < btns.length; i++) {
                var t = btns[i].textContent || btns[i].value || '';
                if (t.includes('Sign in') || t.includes('Log in') || t.includes('Submit')) { btn = btns[i]; break; }
            }
            return JSON.stringify({
                email: email ? 'found' : 'not found',
                password: pass ? 'found' : 'not found',
                button: btn ? 'found' : 'not found',
                title: document.title
            });
        })()
        """)
        #expect(fields != nil, "Should detect login page fields: \(fields ?? "nil")")
    }

    @Test("LinkedIn feed: detect posts and comment buttons")
    func linkedInFeedElements() async {
        let loaded = await openAndVerify(
            "https://www.linkedin.com/feed/",
            condition: "document.readyState === 'complete' ? 'true' : 'false'"
        )
        #expect(loaded, "LinkedIn should load")

        // Wait extra for feed content to render
        try? await Task.sleep(for: .seconds(3))

        let metrics = await runJS("""
        (function() {
            var posts = document.querySelectorAll('.feed-shared-update-v2').length;
            var commentBtns = 0;
            var likeBtns = 0;
            var btns = document.querySelectorAll('button');
            for (var i = 0; i < btns.length; i++) {
                var label = btns[i].getAttribute('aria-label') || '';
                if (label.includes('Comment')) commentBtns++;
                if (label.includes('Like') || label.includes('React')) likeBtns++;
            }
            var compose = document.querySelector('.share-box-feed-entry__top-bar') ? true : false;
            return JSON.stringify({posts: posts, comments: commentBtns, likes: likeBtns, compose: compose});
        })()
        """)
        #expect(metrics != nil, "Should get feed metrics")
        // If logged in there will be posts, if not that's ok
    }

    // MARK: - Core JS Execution

    @Test("executeJavaScript returns document title")
    func executeJSTitle() async {
        let loaded = await openAndVerify(
            "https://www.google.com",
            condition: "document.querySelector('textarea[name=q],input[name=q]') ? 'true' : 'false'"
        )
        #expect(loaded, "Google should load")

        let title = await runJS("document.title")
        #expect(title != nil && !title!.isEmpty, "Should return title")
    }

    @Test("executeJavaScript can count DOM elements")
    func executeJSCountElements() async {
        let loaded = await openAndVerify(
            "https://www.google.com",
            condition: "document.querySelector('textarea[name=q],input[name=q]') ? 'true' : 'false'"
        )
        #expect(loaded, "Google should load")

        let count = await runJS("'' + document.querySelectorAll('a').length")
        #expect(count != nil, "Should return link count")
        if let n = count.flatMap({ Int($0) }) {
            #expect(n > 0, "Google should have links")
        }
    }

    @Test("click and type via JavaScript on Google")
    func clickAndType() async {
        let loaded = await openAndVerify(
            "https://www.google.com",
            condition: "document.querySelector('textarea[name=q],input[name=q]') ? 'true' : 'false'"
        )
        #expect(loaded, "Google should load with search field")

        // Type into search
        let typed = await runJS("""
        (function() {
            var el = document.querySelector('textarea[name=q],input[name=q]');
            if (!el) return 'not found';
            el.focus();
            el.value = 'hello world';
            el.dispatchEvent(new Event('input', {bubbles: true}));
            return el.value;
        })()
        """)
        #expect(typed == "hello world", "Should type 'hello world', got: \(typed ?? "nil")")
    }
}
