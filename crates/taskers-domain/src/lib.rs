pub mod attention;
pub mod ids;
pub mod layout;
pub mod model;
pub mod signal;

pub use attention::AttentionState;
pub use ids::{PaneId, SessionId, WindowId, WorkspaceId};
pub use layout::{LayoutNode, SplitAxis};
pub use model::{
    ActivityItem, AppModel, DomainError, NotificationItem, PaneKind, PaneMetadata,
    PaneMetadataPatch, PaneRecord, PersistedSession, SESSION_SCHEMA_VERSION, WindowRecord,
    Workspace, WorkspaceSummary,
};
pub use signal::{SignalEvent, SignalKind};
