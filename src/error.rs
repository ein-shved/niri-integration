//! Collection of own error-related types

use neovim_lib;
use std::{fmt, io};
use daemonize;
use serde_json;
use regex;

/// Own error type
///
/// This is enum of all error types used by dependencies of this project
#[derive(Debug)]
pub enum Error {
    /// An [io::Error] variant
    Io(io::Error),
    /// A [neovim_lib::CallError] variant
    Neovim(neovim_lib::CallError),
    /// A [niri_ipc::Reply] variant
    Str(String),
    /// A [daemonize::Error] variant
    Daemonize(daemonize::Error),
    /// A [serde_json::Error]
    Json(serde_json::Error),
    /// A [regex::Error]
    Regex(regex::Error)

}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Io(ref e) => e.fmt(f),
            Error::Neovim(ref e) => e.fmt(f),
            Error::Str(ref e) => e.fmt(f),
            Error::Daemonize(ref e) => e.fmt(f),
            Error::Json(ref e) => e.fmt(f),
            Error::Regex(ref e) => e.fmt(f),
        }
    }
}

#[allow(deprecated)]
impl std::error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::Io(ref e) => e.description(),
            Error::Neovim(ref e) => e.description(),
            Error::Str(ref e) => &e,
            Error::Daemonize(ref e) => e.description(),
            Error::Json(ref e) => e.description(),
            Error::Regex(ref e) => e.description(),
        }
    }
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<Error> for io::Error {
    fn from(value: Error) -> Self {
        match value {
            Error::Io(io) => io,
            _ => io::Error::new(io::ErrorKind::InvalidData, value),
        }
    }
}

impl From<neovim_lib::CallError> for Error {
    fn from(value: neovim_lib::CallError) -> Self {
        Self::Neovim(value)
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Self::Str(String::from(value))
    }
}

impl From<daemonize::Error> for Error {
    fn from(value: daemonize::Error) -> Self {
        Self::Daemonize(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<regex::Error> for Error {
    fn from(value: regex::Error) -> Self {
        Self::Regex(value)
    }
}

/// Own result type
///
/// This is result based on [Error]
pub type Result<T> = std::result::Result<T, Error>;
