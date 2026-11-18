//! Host operating system integrations. The portable launcher supports Windows x64.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(not(windows))]
compile_error!("HH Clicker Launcher currently supports Windows x64 only");
