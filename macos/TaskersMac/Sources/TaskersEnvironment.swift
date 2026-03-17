import Foundation

enum TaskersEnvironment {
    private static let inheritedTerminalEnvironmentKeys = [
        "TERM",
        "TERMINFO",
        "TERMINFO_DIRS",
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
        "COLORTERM",
        "NO_COLOR",
        "CLICOLOR",
        "CLICOLOR_FORCE",
        "KITTY_INSTALLATION_DIR",
        "KITTY_LISTEN_ON",
        "KITTY_PUBLIC_KEY",
        "KITTY_WINDOW_ID",
        "GHOSTTY_BIN_DIR",
        "GHOSTTY_RESOURCES_DIR",
        "GHOSTTY_SHELL_FEATURES",
        "GHOSTTY_SHELL_INTEGRATION_XDG_DIR",
    ]

    static var isSmokeTestEnabled: Bool {
        ProcessInfo.processInfo.environment["TASKERS_SMOKE_TEST"] == "1"
    }

    static func scrubInheritedTerminalEnvironment() {
        for key in inheritedTerminalEnvironmentKeys {
            unsetenv(key)
        }
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
