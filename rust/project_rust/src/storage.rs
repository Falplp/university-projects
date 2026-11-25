use crate::event_source::AgentEvent;
use rusqlite::{Connection, Result, params};
use std::path::Path;

pub struct Storage {
    connection: Connection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub timestamp: String,
    pub agent: String,
    pub kind: String,
    pub message: String,
    pub tokens: u32,
    pub duration_ms: u64,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        let storage = Self { connection };
        storage.create_schema()?;
        Ok(storage)
    }

    #[cfg(test)]
    pub fn memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        let storage = Self { connection };
        storage.create_schema()?;
        Ok(storage)
    }

    fn create_schema(&self) -> Result<()> {
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS pipelines (
                id TEXT PRIMARY KEY,
                workspace TEXT NOT NULL,
                state TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pipeline_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                agent TEXT NOT NULL,
                kind TEXT NOT NULL,
                message TEXT NOT NULL,
                tokens INTEGER NOT NULL DEFAULT 0,
                duration_ms INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS errors (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                pipeline_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                agent TEXT NOT NULL,
                message TEXT NOT NULL
            );",
        )
    }

    pub fn save_pipeline(
        &self,
        id: &str,
        workspace: &str,
        state: &str,
        timestamp: &str,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO pipelines (id, workspace, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(id) DO UPDATE SET workspace = excluded.workspace, state = excluded.state, updated_at = excluded.updated_at",
            params![id, workspace, state, timestamp],
        )?;
        Ok(())
    }

    pub fn record_event(&self, pipeline_id: &str, event: &AgentEvent) -> Result<()> {
        self.connection.execute(
            "INSERT INTO events (pipeline_id, timestamp, agent, kind, message, tokens, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                pipeline_id,
                event.timestamp,
                event.agent,
                event.kind,
                event.message,
                event.tokens,
                event.duration_ms
            ],
        )?;
        Ok(())
    }

    pub fn record_error(&self, pipeline_id: &str, event: &AgentEvent) -> Result<()> {
        self.connection.execute(
            "INSERT INTO errors (pipeline_id, timestamp, agent, message) VALUES (?1, ?2, ?3, ?4)",
            params![pipeline_id, event.timestamp, event.agent, event.message],
        )?;
        Ok(())
    }

    pub fn recent_events(&self, pipeline_id: &str) -> Result<Vec<StoredEvent>> {
        let mut statement = self.connection.prepare(
            "SELECT timestamp, agent, kind, message, tokens, duration_ms
             FROM events WHERE pipeline_id = ?1 ORDER BY id ASC",
        )?;
        let rows = statement.query_map([pipeline_id], |row| {
            Ok(StoredEvent {
                timestamp: row.get(0)?,
                agent: row.get(1)?,
                kind: row.get(2)?,
                message: row.get(3)?,
                tokens: row.get(4)?,
                duration_ms: row.get(5)?,
            })
        })?;
        rows.collect()
    }

    pub fn count_errors(&self, pipeline_id: &str) -> Result<u32> {
        self.connection.query_row(
            "SELECT COUNT(*) FROM errors WHERE pipeline_id = ?1",
            [pipeline_id],
            |row| row.get(0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str) -> AgentEvent {
        AgentEvent {
            timestamp: "14:45:00".into(),
            agent: "code-generator".into(),
            kind: kind.into(),
            message: "terminal.exec pytest -q".into(),
            tokens: 14,
            duration_ms: 320,
        }
    }

    #[test]
    fn creates_pipeline_and_updates_its_state() {
        let storage = Storage::memory().unwrap();
        storage
            .save_pipeline("run-1", "coding-space", "RUNNING", "14:45:00")
            .unwrap();
        storage
            .save_pipeline("run-1", "coding-space", "COMPLETED", "14:46:00")
            .unwrap();
        let events = storage.recent_events("run-1").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn stores_and_reads_event_metrics() {
        let storage = Storage::memory().unwrap();
        storage.record_event("run-1", &event("tool")).unwrap();
        let events = storage.recent_events("run-1").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tokens, 14);
        assert_eq!(events[0].duration_ms, 320);
    }

    #[test]
    fn stores_errors_separately_from_events() {
        let storage = Storage::memory().unwrap();
        let failure = event("error");
        storage.record_event("run-1", &failure).unwrap();
        storage.record_error("run-1", &failure).unwrap();
        assert_eq!(storage.count_errors("run-1").unwrap(), 1);
        assert_eq!(storage.recent_events("run-1").unwrap().len(), 1);
    }
}
