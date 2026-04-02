pub mod client;
pub mod controller;
pub mod paths;
pub mod protocol;
pub mod socket;

pub use client::ControlClient;
pub use controller::{ControllerSnapshot, InMemoryController};
pub use paths::default_socket_path;
pub use protocol::{
    BrowserControlCommand, BrowserGetCommand, BrowserLoadState, BrowserPredicateCommand,
    BrowserTarget, BrowserWaitCondition, ControlCommand, ControlError, ControlErrorCode,
    ControlQuery, ControlResponse, IdentifyContext, IdentifyResult, RequestFrame, ResponseFrame,
    TerminalDebugCommand, TerminalDebugResult, TerminalRenderStats, VcsCommand, VcsCommandResult,
    VcsFileEntry, VcsFileStatus, VcsMode, VcsPullRequestInfo, VcsRefEntry, VcsSnapshot,
};
pub use socket::{bind_socket, serve, serve_with_handler};
