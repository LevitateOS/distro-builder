//! Build utilities for creating distribution images.
//!
//! This module provides:
//! - [`context`] - Build context and distro configuration traits
//! - [`filesystem`] - FHS directory structure utilities
//! - [`kernel`] - Kernel building and installation

pub mod context;
pub mod filesystem;
pub mod kernel;
pub mod licenses;
