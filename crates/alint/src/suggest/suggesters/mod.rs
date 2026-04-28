//! Per-family suggesters. Each module exposes a single
//! `propose(scan, progress) -> Vec<Proposal>` entrypoint;
//! `suggest::run` concatenates their output and dedups.

pub mod antipattern;
pub mod bundled;
pub mod todo_age;
