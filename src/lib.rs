// Stylistic lints that do not help this port.
#![allow(unknown_lints, clippy::collapsible_match, clippy::box_default, clippy::type_complexity, clippy::too_many_arguments, clippy::collapsible_if, clippy::collapsible_else_if, clippy::needless_range_loop, clippy::manual_range_contains, clippy::new_without_default)]

//! AVDump3 — reads each file once and feeds the data to any number of hash and container
//! parsers in parallel, then emits metadata reports.
//!
//! This crate is a Rust port of the original C# AVDump3 (AVDump3CL + AVDump3Lib).

pub mod hashes;
pub mod info;
pub mod misc;
pub mod processing;
pub mod reporting;
pub mod settings;
pub mod ui;
