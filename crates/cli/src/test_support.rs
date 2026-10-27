//! Shared test helpers for the CLI unit tests.
//!
//! `std::env` is process-global and Rust runs unit tests in threads, so any
//! test that sets or reads an environment variable must hold [`ENV_LOCK`].
//! It lives here (not in one test module) so tests in different modules that
//! touch the same variables (`build_id`, `config_render`) serialize together.

use std::sync::Mutex;

pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());
