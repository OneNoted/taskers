import AppKit
import Foundation
import WebKit

final class TaskersBrowserView: NSView, TaskersHostedSurface, WKNavigationDelegate {
    let workspaceID: String
    let paneID: String
    let surfaceID: String

    private let core: TaskersCoreBridge
    private let descriptor: TaskersSurfaceDescriptor
    private let backButton = NSButton()
    private let forwardButton = NSButton()
    private let reloadButton = NSButton()
    private let addressField = NSSearchField()
    private let webView: WKWebView
    private var titleObservation: NSKeyValueObservation?
    private var urlObservation: NSKeyValueObservation?
    private var canGoBackObservation: NSKeyValueObservation?
    private var canGoForwardObservation: NSKeyValueObservation?
    private var didRequestInitialFocus = false
    private var lastReportedTitle: String?
    private var lastReportedURL: String?
    private var isDisposed = false

    override var acceptsFirstResponder: Bool {
        true
    }

    var hostingView: NSView { self }

    init(
        core: TaskersCoreBridge,
        workspaceID: String,
        paneID: String,
        surfaceID: String,
        descriptor: TaskersSurfaceDescriptor
    ) {
        self.core = core
        self.workspaceID = workspaceID
        self.paneID = paneID
        self.surfaceID = surfaceID
        self.descriptor = descriptor
        self.webView = WKWebView(frame: .zero, configuration: WKWebViewConfiguration())
        self.lastReportedTitle = Self.trimmed(descriptor.title)
        self.lastReportedURL = Self.trimmed(descriptor.url)

        super.init(frame: NSRect(x: 0, y: 0, width: 640, height: 420))
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
        layer?.backgroundColor = NSColor.windowBackgroundColor.cgColor

        configureChrome()
        observeWebView()
        loadInitialURLIfNeeded()
    }

    required init?(coder: NSCoder) {
        return nil
    }

    deinit {
        dispose()
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard !didRequestInitialFocus, window != nil, Self.resolvedURL(from: descriptor.url) == nil else {
            return
        }

        didRequestInitialFocus = true
        DispatchQueue.main.async { [weak self] in
            guard let self, let window = self.window else {
                return
            }
            window.makeFirstResponder(self.addressField)
        }
    }

    func dispose() {
        guard !isDisposed else {
            return
        }

        isDisposed = true
        titleObservation = nil
        urlObservation = nil
        canGoBackObservation = nil
        canGoForwardObservation = nil
        webView.navigationDelegate = nil
    }

    static func resolvedURL(from rawValue: String?) -> URL? {
        guard let trimmed = trimmed(rawValue), !trimmed.isEmpty else {
            return nil
        }
        if hasExplicitScheme(trimmed) {
            return URL(string: trimmed)
        }
        if trimmed.contains(where: { $0.isWhitespace }) {
            var components = URLComponents(string: "https://duckduckgo.com/")
            components?.queryItems = [URLQueryItem(name: "q", value: trimmed)]
            return components?.url
        }
        if isLocalTarget(trimmed) {
            return URL(string: "http://\(trimmed)")
        }
        return URL(string: "https://\(trimmed)")
    }

    private func configureChrome() {
        let toolbar = NSStackView()
        toolbar.translatesAutoresizingMaskIntoConstraints = false
        toolbar.orientation = .horizontal
        toolbar.alignment = .centerY
        toolbar.spacing = 8
        toolbar.edgeInsets = NSEdgeInsets(top: 10, left: 12, bottom: 10, right: 12)

        configureButton(backButton, symbolName: "chevron.left", action: #selector(goBack))
        configureButton(forwardButton, symbolName: "chevron.right", action: #selector(goForward))
        configureButton(reloadButton, symbolName: "arrow.clockwise", action: #selector(reloadPage))

        addressField.translatesAutoresizingMaskIntoConstraints = false
        addressField.placeholderString = "Enter URL or search"
        addressField.sendsSearchStringImmediately = false
        addressField.target = self
        addressField.action = #selector(loadAddressFromField)
        addressField.stringValue = descriptor.url ?? ""

        toolbar.addArrangedSubview(backButton)
        toolbar.addArrangedSubview(forwardButton)
        toolbar.addArrangedSubview(reloadButton)
        toolbar.addArrangedSubview(addressField)

        webView.translatesAutoresizingMaskIntoConstraints = false
        webView.navigationDelegate = self
        webView.allowsBackForwardNavigationGestures = true

        addSubview(toolbar)
        addSubview(webView)

        NSLayoutConstraint.activate([
            toolbar.leadingAnchor.constraint(equalTo: leadingAnchor),
            toolbar.trailingAnchor.constraint(equalTo: trailingAnchor),
            toolbar.topAnchor.constraint(equalTo: topAnchor),

            webView.leadingAnchor.constraint(equalTo: leadingAnchor),
            webView.trailingAnchor.constraint(equalTo: trailingAnchor),
            webView.topAnchor.constraint(equalTo: toolbar.bottomAnchor),
            webView.bottomAnchor.constraint(equalTo: bottomAnchor),

            addressField.widthAnchor.constraint(greaterThanOrEqualToConstant: 280)
        ])

        updateChrome()
    }

    private func configureButton(_ button: NSButton, symbolName: String, action: Selector) {
        button.translatesAutoresizingMaskIntoConstraints = false
        button.bezelStyle = .texturedRounded
        button.target = self
        button.action = action
        if let image = NSImage(systemSymbolName: symbolName, accessibilityDescription: nil) {
            button.image = image
        }
    }

    private func observeWebView() {
        titleObservation = webView.observe(\.title, options: [.initial, .new]) { [weak self] _, _ in
            self?.syncBrowserMetadata()
        }
        urlObservation = webView.observe(\.url, options: [.initial, .new]) { [weak self] _, _ in
            self?.syncBrowserMetadata()
        }
        canGoBackObservation = webView.observe(\.canGoBack, options: [.initial, .new]) { [weak self] _, _ in
            self?.updateChrome()
        }
        canGoForwardObservation = webView.observe(\.canGoForward, options: [.initial, .new]) { [weak self] _, _ in
            self?.updateChrome()
        }
    }

    private func loadInitialURLIfNeeded() {
        guard let url = Self.resolvedURL(from: descriptor.url) else {
            return
        }
        load(url)
    }

    private func syncBrowserMetadata() {
        updateChrome()

        let title = Self.trimmed(webView.title)
        let currentURL = Self.trimmed(webView.url?.absoluteString)
        guard title != lastReportedTitle || currentURL != lastReportedURL else {
            return
        }

        lastReportedTitle = title
        lastReportedURL = currentURL
        try? core.updateSurfaceMetadata(surfaceId: surfaceID, title: title, url: currentURL)
    }

    private func updateChrome() {
        backButton.isEnabled = webView.canGoBack
        forwardButton.isEnabled = webView.canGoForward
        reloadButton.isEnabled = true
        if let url = webView.url?.absoluteString, !url.isEmpty {
            addressField.stringValue = url
        } else if addressField.stringValue.isEmpty {
            addressField.stringValue = descriptor.url ?? ""
        }
    }

    private func load(_ url: URL) {
        addressField.stringValue = url.absoluteString
        webView.load(URLRequest(url: url))
        lastReportedURL = url.absoluteString
        try? core.updateSurfaceMetadata(surfaceId: surfaceID, url: url.absoluteString)
    }

    @objc private func goBack(_ sender: Any?) {
        _ = sender
        webView.goBack()
    }

    @objc private func goForward(_ sender: Any?) {
        _ = sender
        webView.goForward()
    }

    @objc private func reloadPage(_ sender: Any?) {
        _ = sender
        if webView.url == nil, let url = Self.resolvedURL(from: addressField.stringValue) {
            load(url)
        } else {
            webView.reload()
        }
    }

    @objc private func loadAddressFromField(_ sender: Any?) {
        _ = sender
        guard let url = Self.resolvedURL(from: addressField.stringValue) else {
            return
        }
        load(url)
    }

    private static func trimmed(_ value: String?) -> String? {
        guard let value = value?.trimmingCharacters(in: .whitespacesAndNewlines),
              !value.isEmpty else {
            return nil
        }
        return value
    }

    private static func hasExplicitScheme(_ value: String) -> Bool {
        if value.contains("://") {
            return true
        }
        guard let colonIndex = value.firstIndex(of: ":") else {
            return false
        }
        let scheme = String(value[..<colonIndex])
        let rest = value[value.index(after: colonIndex)...]
        guard let first = scheme.first, first.isLetter else {
            return false
        }
        guard scheme.allSatisfy({ character in
            character.isLetter
                || character.isNumber
                || character == Character("+")
                || character == Character("-")
                || character == Character(".")
        }) else {
            return false
        }
        if rest.hasPrefix("//") {
            return true
        }
        return ["about", "data", "file", "javascript", "mailto"].contains(scheme.lowercased())
    }

    private static func isLocalTarget(_ value: String) -> Bool {
        let lowercased = value.lowercased()
        if lowercased.hasPrefix("localhost")
            || lowercased.hasPrefix("127.0.0.1")
            || lowercased.hasPrefix("[::1]") {
            return true
        }
        if value.contains("/") {
            return false
        }
        guard let separator = value.lastIndex(of: ":") else {
            return false
        }
        let host = String(value[..<separator])
        let port = String(value[value.index(after: separator)...])
        guard !host.isEmpty, !host.contains("."), !host.contains(":") else {
            return false
        }
        return !port.isEmpty && port.allSatisfy({ $0.isNumber })
    }
}
