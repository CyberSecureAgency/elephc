//! Purpose:
//! Groups the generator integration-test modules into a single namespace
//! under `tests::codegen::generators`. Each submodule covers a distinct
//! feature axis so failures localise quickly.
//!
//! Called from:
//!  - `cargo test` through the integration test harness via
//!    `tests/codegen/mod.rs`.

mod arithmetic;
mod basic;
mod control_flow;
mod get_return;
mod interop;
mod send_throw;
mod yield_from;
