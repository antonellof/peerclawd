//! Database layer using redb for persistent storage.

use redb::{Database as RedbDatabase, ReadableTable, TableDefinition};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

// Table definitions
const PEERS: TableDefinition<&str, &[u8]> = TableDefinition::new("peers");
const WALLET: TableDefinition<&str, &[u8]> = TableDefinition::new("wallet");
const AGENTS: TableDefinition<&str, &[u8]> = TableDefinition::new("agents");
const TOOLS: TableDefinition<&str, &[u8]> = TableDefinition::new("tools");
const SETTINGS: TableDefinition<&str, &[u8]> = TableDefinition::new("settings");

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Database error: {0}")]
    Redb(#[from] redb::Error),

    #[error("Database error: {0}")]
    Database(#[from] redb::DatabaseError),

    #[error("Table error: {0}")]
    Table(#[from] redb::TableError),

    #[error("Storage error: {0}")]
    Storage(#[from] redb::StorageError),

    #[error("Transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),

    #[error("Commit error: {0}")]
    Commit(#[from] redb::CommitError),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

/// Database handle for persistent storage.
pub struct Database {
    db: Arc<RedbDatabase>,
}

#[allow(clippy::result_large_err)]
impl Database {
    /// Open or create a database at the given path.
    pub fn open(path: &Path) -> Result<Self, DatabaseError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DatabaseError::Storage(redb::StorageError::Io(e))
            })?;
        }

        let db = RedbDatabase::create(path)?;

        // Initialize tables
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(PEERS)?;
            let _ = write_txn.open_table(WALLET)?;
            let _ = write_txn.open_table(AGENTS)?;
            let _ = write_txn.open_table(TOOLS)?;
            let _ = write_txn.open_table(SETTINGS)?;
        }
        write_txn.commit()?;

        Ok(Self { db: Arc::new(db) })
    }

    // =========================================================================
    // Peers
    // =========================================================================

    /// Store peer information.
    pub fn store_peer<T: Serialize>(&self, peer_id: &str, info: &T) -> Result<(), DatabaseError> {
        let bytes = rmp_serde::to_vec(info)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(PEERS)?;
            table.insert(peer_id, bytes.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get peer information.
    pub fn get_peer<T: DeserializeOwned>(&self, peer_id: &str) -> Result<Option<T>, DatabaseError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(PEERS)?;

        match table.get(peer_id)? {
            Some(bytes) => {
                let value: T = rmp_serde::from_slice(bytes.value())
                    .map_err(|e| DatabaseError::Deserialization(e.to_string()))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// List all peer IDs.
    pub fn list_peer_ids(&self) -> Result<Vec<String>, DatabaseError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(PEERS)?;

        let mut ids = Vec::new();
        for entry in table.iter()? {
            let (key, _) = entry?;
            ids.push(key.value().to_string());
        }
        Ok(ids)
    }

    /// Delete a peer.
    pub fn delete_peer(&self, peer_id: &str) -> Result<(), DatabaseError> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(PEERS)?;
            table.remove(peer_id)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    // =========================================================================
    // Agents
    // =========================================================================

    /// Store agent state.
    pub fn store_agent<T: Serialize>(&self, agent_id: &str, state: &T) -> Result<(), DatabaseError> {
        let bytes = rmp_serde::to_vec(state)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(AGENTS)?;
            table.insert(agent_id, bytes.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get agent state.
    pub fn get_agent<T: DeserializeOwned>(&self, agent_id: &str) -> Result<Option<T>, DatabaseError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS)?;

        match table.get(agent_id)? {
            Some(bytes) => {
                let value: T = rmp_serde::from_slice(bytes.value())
                    .map_err(|e| DatabaseError::Deserialization(e.to_string()))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// List all agent IDs.
    pub fn list_agent_ids(&self) -> Result<Vec<String>, DatabaseError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(AGENTS)?;

        let mut ids = Vec::new();
        for entry in table.iter()? {
            let (key, _) = entry?;
            ids.push(key.value().to_string());
        }
        Ok(ids)
    }

    // =========================================================================
    // Tools
    // =========================================================================

    /// Store WASM tool binary.
    pub fn store_tool(&self, name: &str, wasm_bytes: &[u8]) -> Result<(), DatabaseError> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TOOLS)?;
            table.insert(name, wasm_bytes)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get WASM tool binary.
    pub fn get_tool(&self, name: &str) -> Result<Option<Vec<u8>>, DatabaseError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TOOLS)?;

        match table.get(name)? {
            Some(bytes) => Ok(Some(bytes.value().to_vec())),
            None => Ok(None),
        }
    }

    /// List all tool names.
    pub fn list_tool_names(&self) -> Result<Vec<String>, DatabaseError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TOOLS)?;

        let mut names = Vec::new();
        for entry in table.iter()? {
            let (key, _) = entry?;
            names.push(key.value().to_string());
        }
        Ok(names)
    }

    // =========================================================================
    // Settings
    // =========================================================================

    /// Store a setting.
    pub fn store_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<(), DatabaseError> {
        let bytes = rmp_serde::to_vec(value)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SETTINGS)?;
            table.insert(key, bytes.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Get a setting.
    pub fn get_setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, DatabaseError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SETTINGS)?;

        match table.get(key)? {
            Some(bytes) => {
                let value: T = rmp_serde::from_slice(bytes.value())
                    .map_err(|e| DatabaseError::Deserialization(e.to_string()))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            db: Arc::clone(&self.db),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestPeer {
        name: String,
        score: u32,
    }

    #[test]
    fn test_peer_storage() {
        let dir = tempdir().unwrap();
        let db = Database::open(&dir.path().join("test.redb")).unwrap();

        let peer = TestPeer {
            name: "test-peer".to_string(),
            score: 100,
        };

        db.store_peer("peer-1", &peer).unwrap();

        let loaded: Option<TestPeer> = db.get_peer("peer-1").unwrap();
        assert_eq!(loaded, Some(peer));

        let ids = db.list_peer_ids().unwrap();
        assert_eq!(ids, vec!["peer-1"]);
    }

    #[test]
    fn test_tool_storage() {
        let dir = tempdir().unwrap();
        let db = Database::open(&dir.path().join("test.redb")).unwrap();

        let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d]; // WASM magic

        db.store_tool("test-tool", &wasm_bytes).unwrap();

        let loaded = db.get_tool("test-tool").unwrap();
        assert_eq!(loaded, Some(wasm_bytes));

        let names = db.list_tool_names().unwrap();
        assert_eq!(names, vec!["test-tool"]);
    }
}
