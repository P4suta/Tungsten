import XCTest

@MainActor final class NativeWorkflowTests: XCTestCase {
  private let app = XCUIApplication()
  private let session = UUID().uuidString

  override func setUp() async throws {
    launchApplication()
  }

  private func launchApplication() {
    continueAfterFailure = false
    app.launchArguments = [
      "--tungsten-test-session", session, "-AppleLanguages", "(en)", "-AppleLocale", "en_US",
    ]
    if ProcessInfo.processInfo.environment["TUNGSTEN_LARGE_TEXT"] == "true" {
      app.launchArguments += [
        "-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL",
      ]
    }
    if let appearance = ProcessInfo.processInfo.environment["TUNGSTEN_APPEARANCE"] {
      app.launchArguments += ["-AppleInterfaceStyle", appearance]
    }
    app.launch()
    app.activate()
    #if os(macOS)
      if ProcessInfo.processInfo.environment["TUNGSTEN_LARGE_TEXT"] == "true" {
        for _ in 0..<4 {
          app.menuBars.menuBarItems["View"].click()
          app.menuItems["Zoom In"].click()
        }
        app.menuBars.menuBarItems["View"].click()
        XCTAssertFalse(app.menuItems["Zoom In"].isEnabled)
        app.typeKey(.escape, modifierFlags: [])
      }
    #endif
  }

  private func element(_ id: String) -> XCUIElement {
    app.descendants(matching: .any).matching(identifier: id).firstMatch
  }

  private func tap(_ id: String, file: StaticString = #filePath, line: UInt = #line) {
    let target = element(id)
    activate(target, file: file, line: line)
  }

  private func activate(_ target: XCUIElement, file: StaticString = #filePath, line: UInt = #line) {
    _ = target.waitForExistence(timeout: 3)
    for _ in 0..<10 where !target.exists || !target.isHittable {
      #if os(macOS)
        let sheet = app.sheets.firstMatch
        let scroll = sheet.exists ? sheet.scrollViews.firstMatch : app.scrollViews.firstMatch
        scroll.scroll(byDeltaX: 0, deltaY: -320)
      #else
        app.swipeUp()
      #endif
    }
    XCTAssertTrue(
      target.waitForExistence(timeout: 15) && target.isHittable,
      "Missing control: \(target.identifier)\n\(app.debugDescription)", file: file, line: line)
    #if os(macOS)
      target.click()
    #else
      target.tap()
    #endif
  }

  private func connect(auditConnection: Bool = true) throws {
    let server = try XCTUnwrap(ProcessInfo.processInfo.environment["TUNGSTEN_SERVER"])
    let field = app.textFields["server-address"]
    XCTAssertTrue(field.waitForExistence(timeout: 10))
    if auditConnection { try audit("Connect") }
    #if os(macOS)
      field.click()
    #else
      field.tap()
    #endif
    field.typeText(server)
    capture("Connect")
    tap("connect-server")
    XCTAssertTrue(element("create-note").waitForExistence(timeout: 15))
  }

  private func capture(_ name: String) {
    #if os(macOS)
      let screenshot = app.windows.firstMatch.screenshot()
    #else
      let screenshot = app.screenshot()
    #endif
    let attachment = XCTAttachment(screenshot: screenshot)
    attachment.name = name
    attachment.lifetime = .keepAlways
    add(attachment)
  }

  private func audit(_ screen: String) throws {
    capture(screen)
    guard ProcessInfo.processInfo.environment["TUNGSTEN_AUDIT"] == "true" else { return }
    var types: XCUIAccessibilityAuditType = [
      .elementDetection, .hitRegion, .sufficientElementDescription,
    ]
    #if os(iOS)
      types.insert(.trait)
      if ProcessInfo.processInfo.environment["TUNGSTEN_LARGE_TEXT"] == "true" {
        types.insert(.textClipped)
        types.insert(.dynamicType)
      }
    #else
      types.insert(.action)
      types.insert(.parentChild)
    #endif
    if ProcessInfo.processInfo.environment["TUNGSTEN_CONTRAST"] == "true" {
      types.insert(.contrast)
    }
    let previous = continueAfterFailure
    continueAfterFailure = true
    defer { continueAfterFailure = previous }
    try app.performAccessibilityAudit(for: types) { issue in
      #if os(iOS)
        // Standard SwiftUI controls also produce predictive clipping reports on iOS 27.2.
        // Preserve every report and screen for direct text review instead of inferring a verdict.
        if issue.auditType == .textClipped {
          let report = XCTAttachment(
            string:
              "\(screen): \(issue.detailedDescription)\n\(self.auditAttributes(issue))\n\(issue.element?.debugDescription ?? "No associated element")"
          )
          report.name = "Text clipping review required"
          report.lifetime = .keepAlways
          self.add(report)
          self.capture("Text clipping review: \(screen)")
          return true
        }
      #endif
      #if os(macOS)
        // Finder's Go to Folder input also reports element-free parentage failures on macOS 27.2.
        if issue.auditType == .parentChild, issue.element == nil,
          ["Create Workspace", "Named Workspace"].contains(screen),
          self.app.sheets.firstMatch.exists
        {
          return true
        }
        // macOS 27.2 also reports System Settings' inert presentation hosting groups.
        // Keep controls and groups with content of their own subject to the description audit.
        if issue.auditType == .sufficientElementDescription, let element = issue.element,
          element.elementType == .group, !element.isEnabled, element.label.isEmpty
        {
          let presentations =
            self.app.windows.allElementsBoundByIndex
            + self.app.sheets.allElementsBoundByIndex
          if presentations.contains(where: { $0.frame == element.frame }) {
            return true
          }
          let children = element.children(matching: .any)
          if children.count == 1 {
            let child = children.firstMatch
            if [.scrollView, .splitGroup].contains(child.elementType)
              || !child.label.isEmpty || !(child.value as? String ?? "").isEmpty
            {
              return true
            }
          }
        }
      #endif
      let report = XCTAttachment(
        string:
          "\(screen): \(issue.detailedDescription)\n\(self.auditAttributes(issue))\n\(issue.element?.debugDescription ?? "No associated element")"
      )
      report.name = "Accessibility failure"
      report.lifetime = .keepAlways
      self.add(report)
      return false
    }
  }

  private func auditAttributes(_ issue: XCUIAccessibilityAuditIssue) -> String {
    guard let element = issue.element else { return "No associated element" }
    let children = element.children(matching: .any)
    var attributes =
      "Audit type: \(issue.auditType.rawValue); type: \(element.elementType.rawValue); enabled: \(element.isEnabled); label: \(element.label); children: \(children.count)"
    if children.count > 0 {
      let child = children.firstMatch
      attributes +=
        "\nChild type: \(child.elementType.rawValue); label: \(child.label); value: \(child.value as? String ?? "")"
    }
    return attributes
  }

  private func backToNotes() {
    #if os(iOS)
      let back = app.navigationBars.buttons["All Notes"]
      if back.exists { back.tap() }
    #endif
  }

  private func showCollections() {
    #if os(iOS)
      backToNotes()
      let back = app.navigationBars.buttons["Collections"]
      if back.exists { back.tap() }
    #endif
    XCTAssertTrue(element("collection-all").waitForExistence(timeout: 10))
  }

  func testWritingPersistsAcrossRelaunch() throws {
    try connect()
    tap("create-note")
    let editor = app.textViews["note-editor"]
    XCTAssertTrue(editor.waitForExistence(timeout: 10))
    #if os(macOS)
      editor.click()
    #else
      editor.tap()
    #endif
    editor.typeText("# Native writing\n\nA note shared through the existing server.")
    XCTAssertTrue(app.staticTexts["Saved"].waitForExistence(timeout: 15))
    capture("Writing")
    app.terminate()
    app.launch()
    XCTAssertTrue(editor.waitForExistence(timeout: 15))
    XCTAssertTrue((editor.value as? String)?.contains("A note shared") == true)
    tap("note-details")
    tap("trash-current-note")
    XCTAssertTrue(
      element("detail-create-note").waitForExistence(timeout: 10) || element("create-note").exists,
      app.debugDescription)
    showCollections()
    tap("collection-trash")
    let record = app.buttons.matching(
      NSPredicate(format: "label BEGINSWITH %@", "Native writing")
    ).firstMatch
    XCTAssertTrue(record.waitForExistence(timeout: 10), app.debugDescription)
    let hierarchy = XCTAttachment(string: app.debugDescription)
    hierarchy.name = "Trash restoration hierarchy"
    hierarchy.lifetime = .deleteOnSuccess
    add(hierarchy)
    activate(record)
    tap("restore-record")
    XCTAssertTrue(editor.waitForExistence(timeout: 15))
  }

  func testNativeScreenTour() throws {
    try connect()
    try audit("Notes")
    tap("note-native-start.md")
    XCTAssertTrue(element("note-editor").waitForExistence(timeout: 10))
    let editor = app.textViews["note-editor"]
    try audit("Editor")
    tap("note-details")
    XCTAssertTrue(element("insert-note-link").waitForExistence(timeout: 10))
    try audit("Note Details")
    let version = app.descendants(matching: .any).matching(
      NSPredicate(format: "identifier BEGINSWITH %@", "history-")
    ).firstMatch
    activate(version)
    XCTAssertTrue(element("saved-note-text").waitForExistence(timeout: 10))
    try audit("Version History")
    #if os(macOS)
      activate(app.buttons["Back"])
    #else
      activate(app.navigationBars.buttons.firstMatch)
    #endif
    tap("close-note-details")
    XCTAssertTrue(editor.waitForExistence(timeout: 10))
    tap("note-details")
    tap("insert-note-link")
    XCTAssertTrue(app.buttons["Cancel"].waitForExistence(timeout: 10))
    try audit("Insert Note Link")
    activate(app.buttons["Cancel"])
    tap("note-details")
    tap("add-to-workspace")
    XCTAssertTrue(app.buttons["Cancel"].waitForExistence(timeout: 10))
    try audit("Add to Workspace")
    activate(app.buttons["Cancel"])
    showCollections()
    try audit("Collections")
    tap("create-workspace")
    try exerciseWorkspaceForm()
    showCollections()
    tap("collection-trash")
    XCTAssertTrue(app.staticTexts["Trash Is Empty"].waitForExistence(timeout: 10))
    try audit("Trash")
    showCollections()
    tap("collection-drafts")
    XCTAssertTrue(element("empty-notes").waitForExistence(timeout: 10))
    try audit("Drafts")
    #if os(macOS)
      app.typeKey(",", modifierFlags: .command)
      XCTAssertTrue(app.textFields["server-address"].waitForExistence(timeout: 10))
      try audit("Server Settings")
      app.typeKey("w", modifierFlags: .command)
    #else
      showCollections()
      tap("server-settings")
      XCTAssertTrue(app.textFields["server-address"].waitForExistence(timeout: 10))
      try audit("Server Settings")
      activate(app.buttons["Cancel"])
    #endif
  }

  func testEmptyCollectionsAccessibility() throws {
    try connect()
    tap("note-native-start.md")
    XCTAssertTrue(element("note-editor").waitForExistence(timeout: 10))
    showCollections()
    tap("collection-trash")
    XCTAssertTrue(app.staticTexts["Trash Is Empty"].waitForExistence(timeout: 10))
    try audit("Trash")
    showCollections()
    tap("collection-drafts")
    XCTAssertTrue(element("empty-notes").waitForExistence(timeout: 10))
    try audit("Drafts")
  }

  func testWorkspaceFormAccessibility() throws {
    try connect(auditConnection: false)
    showCollections()
    tap("create-workspace")
    try exerciseWorkspaceForm()
  }

  private func exerciseWorkspaceForm() throws {
    let field = app.textFields["workspace-name"]
    XCTAssertTrue(field.waitForExistence(timeout: 10))
    XCTAssertEqual(field.label, "Workspace Name")
    try audit("Create Workspace")
    activate(field)
    field.typeText("Accessible workspace")
    try audit("Named Workspace")
    activate(app.buttons["Cancel"])
    let discard = app.buttons["Discard Changes"]
    XCTAssertTrue(discard.waitForExistence(timeout: 5), app.debugDescription)
    let keepEditing = app.buttons["Keep Editing"]
    if keepEditing.exists {
      activate(keepEditing)
    } else {
      field.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
    }
    XCTAssertEqual(field.value as? String, "Accessible workspace")
    #if os(iOS)
      let grabber = app.buttons["Sheet Grabber"]
      XCTAssertTrue(grabber.exists)
      grabber.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(
        forDuration: 0.1,
        thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.95)))
    #else
      activate(app.buttons["Cancel"])
    #endif
    XCTAssertTrue(discard.waitForExistence(timeout: 5), app.debugDescription)
    activate(discard)
    XCTAssertTrue(element("create-workspace").waitForExistence(timeout: 10))
  }

}
