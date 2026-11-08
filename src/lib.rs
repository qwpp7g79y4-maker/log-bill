//! Offline estimate of log costs per pattern.
//!
//! A sample is read where the file is. Lines are masked and grouped. With a
//! window and a price file the caller supplies, the sample becomes a day and
//! a month. This crate opens no network connection.

pub mod diff;
pub mod group;
pub mod indexed;
pub mod mask;
pub mod money;
pub mod pattern;
pub mod read;
pub mod render;
pub mod rules;
