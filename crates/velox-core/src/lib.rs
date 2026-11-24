//! # velox-core
//!
//! The download engine behind Velox.
//!
//! * **Dynamic segmentation** – a file is split into N ranges that are fetched
//!   over independent HTTP/1.1 connections. Whenever a connection finishes its
//!   range it steals half of the largest remaining range, so every connection
//!   stays busy until the very last byte.
//! * **Positional writes** into a pre-allocated `.velox` part file.
//! * **Token-bucket rate limiting** – global and per download.
//! * **HLS** (incl. AES-128) and **DASH** (via ffmpeg) stream capture.
//! * **Crash-safe resume** – progress is persisted to SQLite.

pub mod category;
pub mod engine;
mod hls;
mod http;
pub mod ratelimit;
pub mod settings;
pub mod site;
mod store;
pub mod tools;
pub mod types;
mod util;

pub use category::Category;
pub use engine::{Engine, EngineEvent};
pub use settings::{AfterQueue, Schedule, Settings};
pub use tools::ToolsStatus;
pub use types::*;
