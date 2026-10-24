import XCTest

/// Drives the iOS validation build against the demo servers in scripts/screenshots and saves the website's iPhone
/// screenshots. The appearance comes from the simulator (`xcrun simctl ui <device> appearance light|dark`); files
/// go to $SHOT_DIR, or /tmp/iosshots.
final class SiteScreenshots: XCTestCase {
    private var app: XCUIApplication!
    private let out = ProcessInfo.processInfo.environment["SHOT_DIR"] ?? "/tmp/iosshots"

    override func setUpWithError() throws {
        continueAfterFailure = false
        try FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)
        app = XCUIApplication(bundleIdentifier: "it.debba.discophile.validation")
        app.launchArguments = [
            "--fixture-server", "http://localhost:4542",
            "--fixture-audiobookshelf", "http://localhost:13382",
            "--demo-history", "Mina Sol,Cherry Vale,Kiri,Isla Wren,Nova Reyes,Daisy Cole",
            "--promo",
        ]
        app.terminate()
        app.launch()
        XCTAssertTrue(app.tabBars.buttons["Listen Now"].waitForExistence(timeout: 20))
        sleep(4)
    }

    // MARK: Helpers

    private func shot(_ name: String, settle: UInt32 = 2) throws {
        sleep(settle)
        try XCUIScreen.main.screenshot().pngRepresentation.write(to: URL(fileURLWithPath: "\(out)/\(name).png"))
        try app.debugDescription.write(toFile: "\(out)/\(name).txt", atomically: true, encoding: .utf8)
    }

    private func element(_ prefix: String, _ query: XCUIElementQuery? = nil) -> XCUIElement {
        (query ?? app.descendants(matching: .any)).matching(NSPredicate(format: "label BEGINSWITH %@", prefix)).firstMatch
    }

    private func tap(_ prefix: String, _ query: XCUIElementQuery? = nil, timeout: TimeInterval = 10) {
        let target = element(prefix, query)
        if !target.waitForExistence(timeout: timeout) {
            // Leaves the screen and its accessibility tree behind, to see what was there instead.
            try? XCUIScreen.main.screenshot().pngRepresentation.write(to: URL(fileURLWithPath: "\(out)/missing-\(prefix).png"))
            try? app.debugDescription.write(toFile: "\(out)/missing-\(prefix).txt", atomically: true, encoding: .utf8)
            XCTFail("missing \(prefix)")
            return
        }
        target.tap()
    }

    private func tab(_ name: String) {
        app.tabBars.buttons[name].tap()
        sleep(2)
    }

    private func search(_ text: String) {
        tab("Search")
        let field = app.searchFields.firstMatch.exists ? app.searchFields.firstMatch : app.textFields.firstMatch
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        field.typeText(text)
        sleep(3)
    }

    // MARK: Screens

    func test01ListenNow() throws {
        try shot("listen-now")
    }

    private func openDiscophileAlbum() {
        tab("Library")
        sleep(2)
        tap("Discophile, Mina Sol", app.buttons)
        sleep(3)
    }

    func test02Album() throws {
        openDiscophileAlbum()
        try shot("album", settle: 1)
    }

    func test03NowPlaying() throws {
        openDiscophileAlbum()
        tap("Play", app.buttons)
        sleep(7)
        tap("Open Now Playing")
        sleep(2)
        tap("Pause", app.buttons)
        try shot("now-playing", settle: 2)
    }

    func test04Library() throws {
        tab("Library")
        try shot("library-albums", settle: 4)
        tap("Library section", app.buttons)
        sleep(1)
        tap("Artists", app.buttons)
        try shot("library-artists", settle: 3)
    }

    func test05Search() throws {
        search("dai")
        app.keyboards.buttons["Search"].firstMatch.tap()
        try shot("search", settle: 2)
    }

    func test06Statistics() throws {
        tap("Listening statistics", app.buttons)
        // Opens on the last 30 days.
        try shot("statistics-top", settle: 3)
        app.swipeUp(velocity: .slow)
        try shot("statistics", settle: 2)
    }

    func test07Audiobooks() throws {
        tab("Audiobooks")
        sleep(3)
        tap("Orbit Zero", app.buttons)
        try shot("audiobook", settle: 3)
        tap("Continue listening", app.buttons)
        sleep(5)
        tap("Pause", app.buttons)
        try shot("audiobook-player", settle: 2)
    }

    /// The notification permission prompt that appears with the first reply; Allow keeps it from coming back.
    private func allowNotifications(within seconds: Int) {
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        for _ in 0..<seconds {
            let allow = springboard.buttons["Allow"]
            if allow.exists { allow.tap(); return }
            sleep(1)
        }
    }

    private func ask(_ request: String, wait seconds: Int) {
        tap("Music assistant", app.buttons)
        sleep(2)
        tap("AI provider and model", app.buttons)
        sleep(1)
        tap("On this iPhone", app.buttons)
        sleep(1)
        let field = element("Your request")
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        field.typeText(request)
        sleep(1)
        tap("Send request", app.buttons)
        allowNotifications(within: seconds)
        sleep(5)
    }

    func test08AssistantPlaylist() throws {
        ask("Make me a thirty minute playlist for a rainy Sunday. Nothing I've played this month.", wait: 60)
        try shot("ai-local", settle: 1)
    }

    func test09AssistantSong() throws {
        openDiscophileAlbum()
        tap("Play", app.buttons)
        sleep(3)
        // The assistant button lives in Listen Now's toolbar; the song keeps playing.
        if app.buttons["Close"].exists { app.buttons["Close"].tap() }
        tab("Listen Now")
        ask("What makes Discophile by Mina Sol so catchy? Play me more like it from my library.", wait: 70)
        try shot("ai-chatgpt-songs", settle: 1)
        app.swipeDown(velocity: .slow)
        try shot("ai-chatgpt", settle: 2)
    }
}
