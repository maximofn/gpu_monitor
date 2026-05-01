pub mod model;

pub use model::{Gpu, Memory, Process, ProcessKind, Snapshot, Utilization};

pub const DEFAULT_PORT: u16 = 9123;
pub const DEFAULT_BIND: &str = "127.0.0.1";
pub const API_VERSION: &str = "v1";
