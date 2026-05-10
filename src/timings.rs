//! Purpose:
//! Records optional compile-phase durations and notes for CLI timing output.
//! Keeps timing collection lightweight when the user has not requested `--timings`.
//!
//! Called from:
//! - `crate::pipeline::compile()` around each major compiler phase.
//!
//! Key details:
//! - Disabled timing still accepts calls so pipeline code does not branch around every measurement.

use std::time::{Duration, Instant};

pub(crate) struct CompileTimings {
    enabled: bool,
    started_at: Instant,
    notes: Vec<String>,
    phases: Vec<(&'static str, Duration)>,
}

impl CompileTimings {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            started_at: Instant::now(),
            notes: Vec::new(),
            phases: Vec::new(),
        }
    }

    pub(crate) fn record_since(&mut self, phase: &'static str, started_at: Instant) {
        if self.enabled {
            self.phases.push((phase, started_at.elapsed()));
        }
    }

    pub(crate) fn note(&mut self, note: impl Into<String>) {
        if self.enabled {
            self.notes.push(note.into());
        }
    }

    pub(crate) fn report(&self) {
        if !self.enabled {
            return;
        }

        eprintln!("Compiler timings:");
        for note in &self.notes {
            eprintln!("  {}", note);
        }
        for (phase, duration) in &self.phases {
            eprintln!("  {:<12} {:>8.2} ms", phase, duration.as_secs_f64() * 1000.0);
        }
        eprintln!(
            "  {:<12} {:>8.2} ms",
            "total",
            self.started_at.elapsed().as_secs_f64() * 1000.0
        );
    }
}
