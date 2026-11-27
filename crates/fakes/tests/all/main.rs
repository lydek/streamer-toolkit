//! The integration tests in one binary: each file used to be its own test target, linked separately.
//! Every test binds `127.0.0.1:0` and touches no environment variables or process-wide state, so they share a process.

mod control;
mod faults;
mod metrics;
mod obs;
mod snapshot;
mod twitch;
