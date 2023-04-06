// Copyright © Aptos Foundation
// Parts of the project are originally copyright © Meta Platforms, Inc.
// SPDX-License-Identifier: Apache-2.0

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invariant violation: {0}")]
    InvariantViolation(String),
    #[error("Error accessing {0}: {1}")]
    IO(String, #[source] std::io::Error),
    #[error("Error (de)serializing {0}: {1}")]
    BCS(&'static str, #[source] bcs::Error),
    #[error("Error (de)serializing {0}: {1}")]
    Yaml(String, #[source] serde_yaml::Error),
    #[error("Config is missing expected value: {0}")]
    Missing(&'static str),
    #[error("Failed to validate config: {0}")]
    Validation(String),
    #[error("Unexpected error: {0}")]
    Unexpected(String),
}
