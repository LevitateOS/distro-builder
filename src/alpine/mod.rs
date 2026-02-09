//! Alpine Linux shared infrastructure for AcornOS and IuppiterOS.
//!
//! This module contains code shared between both Alpine-based distributions.
//! Functions accept path parameters or parameterized strings to remain
//! distro-agnostic within the Alpine family.

pub mod busybox;
pub mod context;
pub mod extract;
pub mod filesystem;
pub mod firmware;
pub mod keys;
pub mod modules;
pub mod ssh;
pub mod timing;

pub use context::BuildContext;
pub use extract::ExtractPaths;
pub use timing::Timer;
