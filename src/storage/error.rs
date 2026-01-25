//! Storage error types.

use std::path::PathBuf;

/// Errors that can occur during storage operations.
#[derive(Debug)]
pub enum StorageError {
    /// Failed to open the storage environment.
    Open { path: PathBuf, error: String },
    /// Failed to create a database.
    CreateDb { name: &'static str, error: String },
    /// Failed to begin a transaction.
    Transaction(String),
    /// Failed to read from storage.
    Read(String),
    /// Failed to write to storage.
    Write(String),
    /// Failed to delete from storage.
    Delete(String),
    /// Failed to commit a transaction.
    Commit(String),
    /// Data corruption or deserialization failure.
    Corruption(String),
    /// Storage is not initialized (no genesis block).
    NotInitialized,
    /// Genesis block mismatch (different chain).
    GenesisMismatch {
        expected: String,
        found: String,
    },
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Open { path, error } => {
                write!(f, "failed to open storage at {}: {}", path.display(), error)
            }
            StorageError::CreateDb { name, error } => {
                write!(f, "failed to create database '{}': {}", name, error)
            }
            StorageError::Transaction(e) => write!(f, "transaction error: {}", e),
            StorageError::Read(e) => write!(f, "read error: {}", e),
            StorageError::Write(e) => write!(f, "write error: {}", e),
            StorageError::Delete(e) => write!(f, "delete error: {}", e),
            StorageError::Commit(e) => write!(f, "commit error: {}", e),
            StorageError::Corruption(e) => write!(f, "data corruption: {}", e),
            StorageError::NotInitialized => write!(f, "storage not initialized"),
            StorageError::GenesisMismatch { expected, found } => {
                write!(
                    f,
                    "genesis block mismatch: expected {}, found {}",
                    expected, found
                )
            }
        }
    }
}

impl std::error::Error for StorageError {}

impl From<heed::Error> for StorageError {
    fn from(err: heed::Error) -> Self {
        StorageError::Transaction(err.to_string())
    }
}
