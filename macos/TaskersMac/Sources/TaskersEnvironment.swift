import Foundation

enum TaskersEnvironment {
    static var isSmokeTestEnabled: Bool {
        ProcessInfo.processInfo.environment["TASKERS_SMOKE_TEST"] == "1"
    }

    static func configureBundledPaths() {
        guard let resourceURL = Bundle.main.resourceURL else {
            return
        }

        setPath("GHOSTTY_RESOURCES_DIR", url: resourceURL.appendingPathComponent("ghostty"))
        setPath("TERMINFO", url: resourceURL.appendingPathComponent("terminfo"))
        setPath("TASKERS_CTL_PATH", url: resourceURL.appendingPathComponent("bin/taskersctl"))
    }

    static func defaultCoreOptions() -> TaskersCoreOptions {
        TaskersCoreOptions(
            sessionPath: nil,
            socketPath: nil,
            configuredShell: nil,
            demo: false,
            backend: "ghostty_embedded"
        )
    }

    private static func setPath(_ key: String, url: URL) {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return
        }

        setenv(key, url.path, 1)
    }
}
