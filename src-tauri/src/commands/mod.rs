// Tauri command surface, split by domain. Everything is re-exported flat so lib.rs's
// invoke_handler keeps addressing commands as `commands::<name>` — the split is purely
// organizational, no behavior or command names changed.

mod chats;
mod dashboard;
mod library;
mod recommend;
mod steam_import;
mod system;

pub use chats::*;
pub use dashboard::*;
pub use library::*;
pub use recommend::*;
pub use steam_import::*;
pub use system::*;
