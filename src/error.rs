//! Crate error type, hand-rolled per house convention.

/// Every failure the harness can produce.
#[derive(Debug)]
pub enum Error {
    /// A CLI flag failed to parse.
    InvalidFlag {
        /// The flag as given on the command line.
        flag: String,
        /// Why it was rejected.
        reason: String,
    },
    /// A CLI flag is not recognised.
    UnknownFlag(String),
    /// The gossipsub configuration was rejected by the builder.
    Config(String),
    /// Building the gossipsub behaviour failed.
    Behaviour(&'static str),
    /// Subscribing to the benchmark topic failed.
    Subscribe(String),
    /// Listening on a memory address failed.
    Listen(String),
    /// An internal channel closed before the run finished.
    ChannelClosed(&'static str),
    /// The tokio runtime could not be built.
    Runtime(std::io::Error),
    /// Writing the report failed.
    Report(std::io::Error),
    /// A setup phase did not complete in time.
    SetupTimeout(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidFlag { flag, reason } => {
                write!(f, "invalid flag {flag}: {reason}")
            }
            Error::UnknownFlag(flag) => write!(f, "unknown flag {flag}"),
            Error::Config(reason) => write!(f, "gossipsub config rejected: {reason}"),
            Error::Behaviour(reason) => write!(f, "gossipsub behaviour rejected: {reason}"),
            Error::Subscribe(reason) => write!(f, "subscribe failed: {reason}"),
            Error::Listen(reason) => write!(f, "listen failed: {reason}"),
            Error::ChannelClosed(which) => write!(f, "channel closed early: {which}"),
            Error::Runtime(e) => write!(f, "tokio runtime: {e}"),
            Error::Report(e) => write!(f, "writing report: {e}"),
            Error::SetupTimeout(phase) => write!(f, "setup phase timed out: {phase}"),
        }
    }
}

impl std::error::Error for Error {}
