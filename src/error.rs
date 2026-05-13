// src/error.rs – framework error type.

use std::fmt;

#[derive(Debug)]
pub enum FrameworkError {
    /// A required path parameter was missing or unparseable.
    ParamParse { name: String, message: String },
    /// A required request body was missing or malformed.
    BodyParse(String),
    /// A required header was missing.
    MissingHeader(String),
    /// A required query parameter was missing.
    MissingQuery(String),
    /// Authentication / authorisation failure.
    Unauthorized,
    /// Generic internal error.
    Internal(String),
}

impl fmt::Display for FrameworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameworkError::ParamParse { name, message } =>
                write!(f, "param '{}': {}", name, message),
            FrameworkError::BodyParse(msg) =>
                write!(f, "body parse error: {}", msg),
            FrameworkError::MissingHeader(h) =>
                write!(f, "missing header: {}", h),
            FrameworkError::MissingQuery(q) =>
                write!(f, "missing query param: {}", q),
            FrameworkError::Unauthorized =>
                write!(f, "unauthorized"),
            FrameworkError::Internal(msg) =>
                write!(f, "internal error: {}", msg),
        }
    }
}

impl std::error::Error for FrameworkError {}
