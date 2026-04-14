import Testing
import Foundation
@testable import Agent_

@Suite("AccessibilityEnabled")
@MainActor
struct AccessibilityEnabledTests {

    let settings = AccessibilityEnabled.shared

    // MARK: - Default State

    @Test("All Accessibility actions enabled by default")
    func allAxEnabledByDefault() {
        // Every known AX ID should be in axEnabled on a fresh state
        for id in AccessibilityEnabledIDs.allAxIds {
            #expect(settings.isAxEnabled(id), "Expected \(id) to be enabled by default")
        }
    }

    @Test("All Apple Events selectors enabled by default")
    func allAeEnabledByDefault() {
        // Every known AE ID should be in aeEnabled on a fresh state
        for id in AppleEventsEnabledIDs.allAeIds {
            #expect(settings.isAeEnabled(id), "Expected \(id) to be enabled by default")
        }
    }

    // MARK: - AX Action: toggle AXPress

    @Test("AX Action: AXPress enabled means not restricted")
    func axActionEnabledNotRestricted() {
        // Ensure AXPress is enabled
        if !settings.isAxEnabled("AXPress") {
            settings.toggleAx("AXPress")
        }
        #expect(settings.isAxEnabled("AXPress"))
        #expect(!settings.isAxRestricted("AXPress"))
    }

    @Test("AX Action: disabling AXPress makes it restricted")
    func axActionDisabledIsRestricted() {
        // Ensure enabled first, then disable
        if !settings.isAxEnabled("AXPress") {
            settings.toggleAx("AXPress")
        }
        settings.toggleAx("AXPress") // disable
        #expect(!settings.isAxEnabled("AXPress"))
        #expect(settings.isAxRestricted("AXPress"))

        // Restore
        settings.toggleAx("AXPress")
    }

    // MARK: - AX Role: toggle AXSecureTextField

    @Test("AX Role: AXSecureTextField enabled means not restricted")
    func axRoleEnabledNotRestricted() {
        if !settings.isAxEnabled("AXSecureTextField") {
            settings.toggleAx("AXSecureTextField")
        }
        #expect(settings.isAxEnabled("AXSecureTextField"))
        #expect(!settings.isAxRestricted("AXSecureTextField"))
    }

    @Test("AX Role: disabling AXSecureTextField makes it restricted")
    func axRoleDisabledIsRestricted() {
        if !settings.isAxEnabled("AXSecureTextField") {
            settings.toggleAx("AXSecureTextField")
        }
        settings.toggleAx("AXSecureTextField") // disable
        #expect(!settings.isAxEnabled("AXSecureTextField"))
        #expect(settings.isAxRestricted("AXSecureTextField"))

        // Restore
        settings.toggleAx("AXSecureTextField")
    }

    // MARK: - AE Write Selector: toggle delete

    @Test("AE Selector: delete enabled means not restricted")
    func aeSelectorEnabledNotRestricted() {
        if !settings.isAeEnabled("delete") {
            settings.toggleAe("delete")
        }
        #expect(settings.isAeEnabled("delete"))
        #expect(!settings.isAeRestricted("delete"))
    }

    @Test("AE Selector: disabling delete makes it restricted")
    func aeSelectorDisabledIsRestricted() {
        if !settings.isAeEnabled("delete") {
            settings.toggleAe("delete")
        }
        settings.toggleAe("delete") // disable
        #expect(!settings.isAeEnabled("delete"))
        #expect(settings.isAeRestricted("delete"))

        // Restore
        settings.toggleAe("delete")
    }

    // MARK: - Toggle is idempotent round-trip

    @Test("AX Toggle round-trip restores original state")
    func axToggleRoundTrip() {
        let id = "AXConfirm"
        let original = settings.isAxEnabled(id)
        settings.toggleAx(id)
        #expect(settings.isAxEnabled(id) != original)
        settings.toggleAx(id)
        #expect(settings.isAxEnabled(id) == original)
    }

    @Test("AE Toggle round-trip restores original state")
    func aeToggleRoundTrip() {
        let selector = "close"
        let original = settings.isAeEnabled(selector)
        settings.toggleAe(selector)
        #expect(settings.isAeEnabled(selector) != original)
        settings.toggleAe(selector)
        #expect(settings.isAeEnabled(selector) == original)
    }

    // MARK: - Unknown IDs

    @Test("Unknown AX ID is NOT restricted - only known IDs can be restricted")
    func unknownAxIdNotRestricted() {
        // Unknown IDs (not in allAxIds) should not be restricted
        // They're just not part of the safety system
        #expect(!settings.isAxRestricted("SomeUnknownAction"))
        #expect(!settings.isAxEnabled("SomeUnknownAction")) // not in the system at all
    }

    @Test("Unknown AE ID is NOT restricted - only known IDs can be restricted")
    func unknownAeIdNotRestricted() {
        #expect(!settings.isAeRestricted("SomeUnknownSelector"))
        #expect(!settings.isAeEnabled("SomeUnknownSelector"))
    }

    // MARK: - All Apple Event Write Selectors

    static let allWriteSelectors = [
        "delete", "close", "remove", "quit", "move",
        "moveTo", "duplicate", "save", "set", "sendMessage"
    ]

    @Test("All write selectors enabled by default")
    func allWriteSelectorsEnabled() {
        for sel in Self.allWriteSelectors {
            #expect(settings.isAeEnabled(sel), "Expected '\(sel)' to be enabled by default")
            #expect(!settings.isAeRestricted(sel), "Expected '\(sel)' to not be restricted when enabled")
        }
    }

    @Test("Each write selector can be disabled and re-enabled")
    func writeSelectorsToggle() {
        for sel in Self.allWriteSelectors {
            // Ensure enabled
            if !settings.isAeEnabled(sel) { settings.toggleAe(sel) }
            #expect(settings.isAeEnabled(sel))

            // Disable
            settings.toggleAe(sel)
            #expect(!settings.isAeEnabled(sel), "'\(sel)' should be disabled after toggle")
            #expect(settings.isAeRestricted(sel), "'\(sel)' should be restricted when disabled")

            // Re-enable
            settings.toggleAe(sel)
            #expect(settings.isAeEnabled(sel), "'\(sel)' should be re-enabled after second toggle")
            #expect(!settings.isAeRestricted(sel), "'\(sel)' should not be restricted when re-enabled")
        }
    }

    @Test("close: enabled = allowed, disabled = blocked")
    func closeSelector() {
        if !settings.isAeEnabled("close") { settings.toggleAe("close") }
        #expect(!settings.isAeRestricted("close"))
        settings.toggleAe("close")
        #expect(settings.isAeRestricted("close"))
        settings.toggleAe("close") // restore
    }

    @Test("remove: enabled = allowed, disabled = blocked")
    func removeSelector() {
        if !settings.isAeEnabled("remove") { settings.toggleAe("remove") }
        #expect(!settings.isAeRestricted("remove"))
        settings.toggleAe("remove")
        #expect(settings.isAeRestricted("remove"))
        settings.toggleAe("remove") // restore
    }

    @Test("quit: enabled = allowed, disabled = blocked")
    func quitSelector() {
        if !settings.isAeEnabled("quit") { settings.toggleAe("quit") }
        #expect(!settings.isAeRestricted("quit"))
        settings.toggleAe("quit")
        #expect(settings.isAeRestricted("quit"))
        settings.toggleAe("quit") // restore
    }

    @Test("move: enabled = allowed, disabled = blocked")
    func moveSelector() {
        if !settings.isAeEnabled("move") { settings.toggleAe("move") }
        #expect(!settings.isAeRestricted("move"))
        settings.toggleAe("move")
        #expect(settings.isAeRestricted("move"))
        settings.toggleAe("move") // restore
    }

    @Test("moveTo: enabled = allowed, disabled = blocked")
    func moveToSelector() {
        if !settings.isAeEnabled("moveTo") { settings.toggleAe("moveTo") }
        #expect(!settings.isAeRestricted("moveTo"))
        settings.toggleAe("moveTo")
        #expect(settings.isAeRestricted("moveTo"))
        settings.toggleAe("moveTo") // restore
    }

    @Test("duplicate: enabled = allowed, disabled = blocked")
    func duplicateSelector() {
        if !settings.isAeEnabled("duplicate") { settings.toggleAe("duplicate") }
        #expect(!settings.isAeRestricted("duplicate"))
        settings.toggleAe("duplicate")
        #expect(settings.isAeRestricted("duplicate"))
        settings.toggleAe("duplicate") // restore
    }

    @Test("save: enabled = allowed, disabled = blocked")
    func saveSelector() {
        if !settings.isAeEnabled("save") { settings.toggleAe("save") }
        #expect(!settings.isAeRestricted("save"))
        settings.toggleAe("save")
        #expect(settings.isAeRestricted("save"))
        settings.toggleAe("save") // restore
    }

    @Test("set: enabled = allowed, disabled = blocked")
    func setSelector() {
        if !settings.isAeEnabled("set") { settings.toggleAe("set") }
        #expect(!settings.isAeRestricted("set"))
        settings.toggleAe("set")
        #expect(settings.isAeRestricted("set"))
        settings.toggleAe("set") // restore
    }

    @Test("sendMessage: enabled = allowed, disabled = blocked")
    func sendMessageSelector() {
        if !settings.isAeEnabled("sendMessage") { settings.toggleAe("sendMessage") }
        #expect(!settings.isAeRestricted("sendMessage"))
        settings.toggleAe("sendMessage")
        #expect(settings.isAeRestricted("sendMessage"))
        settings.toggleAe("sendMessage") // restore
    }

    // MARK: - Non-write method should never be in the enabled set

    @Test("Non-write methods are not known enabled IDs")
    func nonWriteMethodsNotInAllIds() {
        let readMethods = ["playlists", "searchFor", "currentTrack", "accounts",
                           "mailboxes", "folders", "notes", "reminders", "windows",
                           "documents", "paragraphs", "tracks", "name", "artist"]
        for method in readMethods {
            #expect(!AppleEventsEnabledIDs.allAeIds.contains(method),
                    "'\(method)' should NOT be in allAeIds — it's a read method")
        }
    }

    // MARK: - UserDefaults key consistency

    @Test("axEnabledKey matches expected value")
    func axEnabledKeyMatchesInternal() {
        // The key should be the expected value
        #expect(axEnabledKey == "AccessibilityEnabled")
    }

    @Test("aeEnabledKey matches expected value")
    func aeEnabledKeyMatchesInternal() {
        #expect(aeEnabledKey == "AppleEventsEnabled")
    }

    @Test("UserDefaults is populated after first access for Accessibility")
    func userDefaultsPopulatedOnInitAx() {
        // After initializing AccessibilityEnabled.shared, UserDefaults should have the key
        let stored = UserDefaults.standard.stringArray(forKey: axEnabledKey)
        #expect(stored != nil, "UserDefaults should have '\(axEnabledKey)' after init")
        #expect(!stored!.isEmpty, "UserDefaults array should not be empty")
    }

    @Test("UserDefaults is populated after first access for AppleEvents")
    func userDefaultsPopulatedOnInitAe() {
        // After initializing AccessibilityEnabled.shared, UserDefaults should have the key
        let stored = UserDefaults.standard.stringArray(forKey: aeEnabledKey)
        #expect(stored != nil, "UserDefaults should have '\(aeEnabledKey)' after init")
        #expect(!stored!.isEmpty, "UserDefaults array should not be empty")
    }
}