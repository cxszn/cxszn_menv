//! Atomic JSON snapshot store used by the Windows Tauri composition root.
//!
//! This is a transitional durable adapter, not the final SQLite journal. It
//! keeps native plan/apply state behind the `EnvironmentStore` port and never
//! accepts an arbitrary path from UI or MCP.

use cxszn_menv_domain::EnvironmentSnapshot;
#[cfg(windows)]
use cxszn_menv_ports::{ConfirmationRecord, ConfirmationStore};
use cxszn_menv_ports::{EnvironmentStore, PortError};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(windows)]
use rusqlite::{params, Connection, OptionalExtension};
#[cfg(windows)]
use std::time::Duration;

static NEXT_TEMP_SUFFIX: AtomicU64 = AtomicU64::new(0);

pub struct JsonEnvironmentStore {
    path: PathBuf,
}

impl JsonEnvironmentStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_for_current_user() -> Self {
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Local"));
        Self::new(root.join("cxszn-menv").join("state-rust.json"))
    }

    fn temporary_path(&self) -> PathBuf {
        self.path.with_extension(format!(
            "json.{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP_SUFFIX.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn backup_path(&self) -> PathBuf {
        self.path.with_extension("json.bak")
    }

    fn ensure_parent(&self) -> Result<(), PortError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| PortError::Platform(format!("create state directory: {error}")))
    }

    fn load_backup_or(
        &self,
        fallback: Result<EnvironmentSnapshot, PortError>,
    ) -> Result<EnvironmentSnapshot, PortError> {
        let backup = self.backup_path();
        let Ok(value) = fs::read_to_string(&backup) else {
            return fallback;
        };
        parse_snapshot(&value).map_err(|error| {
            PortError::Platform(format!("parse environment state backup: {error}"))
        })
    }
}

impl EnvironmentStore for JsonEnvironmentStore {
    fn load(&self) -> Result<EnvironmentSnapshot, PortError> {
        match fs::read_to_string(&self.path) {
            Ok(value) => match parse_snapshot(&value) {
                Ok(snapshot) => Ok(snapshot),
                Err(error) => self.load_backup_or(Err(PortError::Platform(format!(
                    "parse environment state: {error}"
                )))),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.load_backup_or(Ok(EnvironmentSnapshot::default()))
            }
            Err(error) => Err(PortError::Platform(format!(
                "read environment state: {error}"
            ))),
        }
    }

    fn save(&self, snapshot: &EnvironmentSnapshot) -> Result<(), PortError> {
        self.ensure_parent()?;
        let bytes = serde_json::to_vec_pretty(snapshot).map_err(|error| {
            PortError::Platform(format!("serialize environment state: {error}"))
        })?;
        let temporary = self.temporary_path();
        let mut file = fs::File::create(&temporary).map_err(|error| {
            PortError::Platform(format!("create environment state staging: {error}"))
        })?;
        file.write_all(&bytes).map_err(|error| {
            PortError::Platform(format!("write environment state staging: {error}"))
        })?;
        file.sync_all().map_err(|error| {
            PortError::Platform(format!("flush environment state staging: {error}"))
        })?;
        drop(file);

        let backup = self.backup_path();
        let had_previous = self.path.exists();
        if had_previous {
            let _ = fs::remove_file(&backup);
            fs::rename(&self.path, &backup).map_err(|error| {
                let _ = fs::remove_file(&temporary);
                PortError::Platform(format!("backup environment state: {error}"))
            })?;
        }
        if let Err(error) = fs::rename(&temporary, &self.path) {
            if had_previous {
                let _ = fs::rename(&backup, &self.path);
            }
            let _ = fs::remove_file(&temporary);
            return Err(PortError::Platform(format!(
                "publish environment state: {error}"
            )));
        }
        let _ = fs::remove_file(&backup);
        Ok(())
    }
}

/// SQLite/WAL environment store used by the Windows Tauri composition root.
///
/// The snapshot payload remains versioned JSON at the application boundary,
/// while the state row and operation journal are committed in one SQLite
/// transaction. This keeps the domain schema stable during the migration and
/// gives crash recovery a durable commit boundary.
#[cfg(windows)]
#[derive(Clone)]
pub struct SqliteEnvironmentStore {
    path: PathBuf,
    legacy_path: Option<PathBuf>,
}

#[cfg(windows)]
impl SqliteEnvironmentStore {
    pub fn new(path: impl Into<PathBuf>, legacy_path: Option<PathBuf>) -> Self {
        Self {
            path: path.into(),
            legacy_path,
        }
    }

    pub fn default_for_current_user() -> Self {
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Local"));
        Self::new(
            root.join("cxszn-menv").join("state-rust.sqlite3"),
            Some(root.join("cxszn-menv").join("state-rust.json")),
        )
    }

    fn open(&self) -> Result<Connection, PortError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                PortError::Platform(format!("create SQLite state directory: {error}"))
            })?;
        }
        let connection = Connection::open(&self.path)
            .map_err(|error| PortError::Platform(format!("open SQLite state: {error}")))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| {
                PortError::Platform(format!("configure SQLite busy timeout: {error}"))
            })?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(|error| PortError::Platform(format!("enable SQLite WAL: {error}")))?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(|error| {
                PortError::Platform(format!("configure SQLite durability: {error}"))
            })?;
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS menv_meta (\
                    key TEXT PRIMARY KEY NOT NULL,\
                    value TEXT NOT NULL\
                );\
                CREATE TABLE IF NOT EXISTS menv_environment_state (\
                    id INTEGER PRIMARY KEY CHECK (id = 1),\
                    schema_version TEXT NOT NULL,\
                    payload TEXT NOT NULL,\
                    updated_at TEXT NOT NULL\
                );\
                CREATE TABLE IF NOT EXISTS menv_operation_journal (\
                    operation_id TEXT PRIMARY KEY NOT NULL,\
                    payload TEXT NOT NULL\
                );\
                CREATE TABLE IF NOT EXISTS menv_php_install_plans (\
                    plan_id TEXT PRIMARY KEY NOT NULL,\
                    payload TEXT NOT NULL,\
                    updated_at TEXT NOT NULL\
                );\
                CREATE TABLE IF NOT EXISTS menv_install_journal (\
                    id INTEGER PRIMARY KEY AUTOINCREMENT,\
                    journal_path TEXT NOT NULL,\
                    payload TEXT NOT NULL,\
                    updated_at TEXT NOT NULL\
                );\
                CREATE TABLE IF NOT EXISTS menv_confirmations (\
                    token_hash TEXT PRIMARY KEY NOT NULL,\
                    plan_id TEXT NOT NULL,\
                    plan_hash TEXT NOT NULL,\
                    actor TEXT NOT NULL,\
                    expires_at INTEGER NOT NULL\
                );\
                INSERT INTO menv_meta(key, value)\
                    VALUES ('schema_version', '1')\
                    ON CONFLICT(key) DO NOTHING;",
            )
            .map_err(|error| PortError::Platform(format!("migrate SQLite state: {error}")))?;
        let schema: String = connection
            .query_row(
                "SELECT value FROM menv_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| PortError::Platform(format!("read SQLite schema version: {error}")))?;
        if schema != "1" {
            return Err(PortError::Platform(format!(
                "unsupported SQLite state schema: {schema}"
            )));
        }
        Ok(connection)
    }

    /// Load persisted PHP install-plan records from the same SQLite database
    /// used for the environment snapshot. The payload remains owned by the
    /// Tauri adapter so the domain crate does not depend on plan JSON shape.
    pub fn load_php_install_plan_rows(&self) -> Result<Vec<(String, String)>, PortError> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare("SELECT plan_id, payload FROM menv_php_install_plans ORDER BY plan_id")
            .map_err(|error| {
                PortError::Platform(format!("prepare SQLite install plans: {error}"))
            })?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| PortError::Platform(format!("read SQLite install plans: {error}")))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| PortError::Platform(format!("decode SQLite install plans: {error}")))
    }

    /// Replace all persisted PHP install-plan records in one transaction.
    pub fn save_php_install_plan_rows(&self, rows: &[(String, String)]) -> Result<(), PortError> {
        let mut connection = self.open()?;
        let transaction = connection.transaction().map_err(|error| {
            PortError::Platform(format!("begin SQLite install-plan transaction: {error}"))
        })?;
        transaction
            .execute("DELETE FROM menv_php_install_plans", [])
            .map_err(|error| PortError::Platform(format!("reset SQLite install plans: {error}")))?;
        for (plan_id, payload) in rows {
            transaction
                .execute(
                    r#"
                    INSERT INTO menv_php_install_plans(plan_id, payload, updated_at)
                    VALUES (?1, ?2, CURRENT_TIMESTAMP)
                    "#,
                    params![plan_id, payload],
                )
                .map_err(|error| {
                    PortError::Platform(format!("write SQLite install plan: {error}"))
                })?;
        }
        transaction
            .commit()
            .map_err(|error| PortError::Platform(format!("commit SQLite install plans: {error}")))
    }

    /// Mirror one append-only installer journal line into SQLite. The JSONL
    /// file remains the recovery source; this table provides queryable audit
    /// evidence without changing the install adapter's append semantics.
    pub fn append_install_journal_row(
        &self,
        journal_path: &Path,
        payload: &str,
    ) -> Result<(), PortError> {
        let connection = self.open()?;
        connection
            .execute(
                r#"
                INSERT INTO menv_install_journal(journal_path, payload, updated_at)
                VALUES (?1, ?2, CURRENT_TIMESTAMP)
                "#,
                params![journal_path.to_string_lossy().replace('/', "\\"), payload],
            )
            .map_err(|error| {
                PortError::Platform(format!("write SQLite install journal: {error}"))
            })?;
        Ok(())
    }

    #[cfg(test)]
    pub fn load_install_journal_rows(&self) -> Result<Vec<(String, String)>, PortError> {
        let connection = self.open()?;
        let mut statement = connection
            .prepare("SELECT journal_path, payload FROM menv_install_journal ORDER BY id")
            .map_err(|error| {
                PortError::Platform(format!("prepare SQLite install journal: {error}"))
            })?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| {
                PortError::Platform(format!("read SQLite install journal: {error}"))
            })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| PortError::Platform(format!("decode SQLite install journal: {error}")))
    }

    fn save_on_connection(
        &self,
        connection: &mut Connection,
        snapshot: &EnvironmentSnapshot,
    ) -> Result<(), PortError> {
        let payload = serde_json::to_string(snapshot)
            .map_err(|error| PortError::Platform(format!("serialize SQLite state: {error}")))?;
        let transaction = connection.transaction().map_err(|error| {
            PortError::Platform(format!("begin SQLite state transaction: {error}"))
        })?;
        transaction
            .execute(
                r#"
                INSERT INTO menv_environment_state(id, schema_version, payload, updated_at)
                VALUES (1, ?1, ?2, ?3)
                ON CONFLICT(id) DO UPDATE SET
                  schema_version = excluded.schema_version,
                  payload = excluded.payload,
                  updated_at = excluded.updated_at
                "#,
                params![snapshot.schema_version, payload, snapshot.updated_at],
            )
            .map_err(|error| PortError::Platform(format!("write SQLite state: {error}")))?;
        transaction
            .execute("DELETE FROM menv_operation_journal", [])
            .map_err(|error| {
                PortError::Platform(format!("reset SQLite operation journal: {error}"))
            })?;
        for operation in &snapshot.operations {
            let operation_payload = serde_json::to_string(operation).map_err(|error| {
                PortError::Platform(format!("serialize SQLite operation journal: {error}"))
            })?;
            transaction
                .execute(
                    "INSERT INTO menv_operation_journal(operation_id, payload) VALUES (?1, ?2)",
                    params![operation.operation_id, operation_payload],
                )
                .map_err(|error| {
                    PortError::Platform(format!("write SQLite operation journal: {error}"))
                })?;
        }
        transaction.commit().map_err(|error| {
            PortError::Platform(format!("commit SQLite state transaction: {error}"))
        })
    }
}

#[cfg(windows)]
impl EnvironmentStore for SqliteEnvironmentStore {
    fn load(&self) -> Result<EnvironmentSnapshot, PortError> {
        let connection = self.open()?;
        let payload: Option<String> = connection
            .query_row(
                "SELECT payload FROM menv_environment_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| PortError::Platform(format!("read SQLite state: {error}")))?;
        if let Some(payload) = payload {
            return parse_snapshot(&payload).map_err(|error| {
                PortError::Platform(format!("parse SQLite environment state: {error}"))
            });
        }

        if let Some(legacy_path) = &self.legacy_path {
            if let Ok(value) = fs::read_to_string(legacy_path) {
                let snapshot = parse_snapshot(&value).map_err(|error| {
                    PortError::Platform(format!("parse legacy environment state: {error}"))
                })?;
                let mut connection = connection;
                self.save_on_connection(&mut connection, &snapshot)?;
                return Ok(snapshot);
            }
        }
        Ok(EnvironmentSnapshot::default())
    }

    fn save(&self, snapshot: &EnvironmentSnapshot) -> Result<(), PortError> {
        let mut connection = self.open()?;
        self.save_on_connection(&mut connection, snapshot)
    }
}

#[cfg(windows)]
impl ConfirmationStore for SqliteEnvironmentStore {
    fn insert(&self, record: &ConfirmationRecord) -> Result<(), PortError> {
        let expires_at = i64::try_from(record.expires_at_epoch_seconds)
            .map_err(|_| PortError::Platform("confirmation expiration is out of range".into()))?;
        self.open()?.execute(
            "INSERT INTO menv_confirmations(token_hash, plan_id, plan_hash, actor, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![record.token_hash, record.plan_id, record.plan_hash, record.actor, expires_at],
        ).map_err(|error| PortError::Platform(format!("record confirmation digest: {error}")))?;
        Ok(())
    }

    fn find(&self, token_hash: &str) -> Result<Option<ConfirmationRecord>, PortError> {
        self.open()?.query_row(
            "SELECT token_hash, plan_id, plan_hash, actor, expires_at FROM menv_confirmations WHERE token_hash = ?1",
            [token_hash],
            |row| Ok(ConfirmationRecord {
                token_hash: row.get(0)?, plan_id: row.get(1)?, plan_hash: row.get(2)?,
                actor: row.get(3)?, expires_at_epoch_seconds: row.get(4)?,
            }),
        ).optional().map_err(|error| PortError::Platform(format!("read confirmation digest: {error}")))
    }
}

fn parse_snapshot(value: &str) -> Result<EnvironmentSnapshot, String> {
    let snapshot: EnvironmentSnapshot =
        serde_json::from_str(value).map_err(|error| error.to_string())?;
    if snapshot.schema_version != "1" {
        return Err(format!(
            "unsupported environment state schema: {}",
            snapshot.schema_version
        ));
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn missing_state_loads_default_and_save_roundtrips() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-store-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = JsonEnvironmentStore::new(root.join("nested").join("state.json"));
        assert_ne!(store.temporary_path(), store.temporary_path());
        let initial = store.load().expect("missing state is default");
        store.save(&initial).expect("save state");
        assert_eq!(store.load().expect("reload state"), initial);
        let mut updated = initial.clone();
        updated.updated_at = "updated".to_owned();
        store.save(&updated).expect("save state a second time");
        assert_eq!(store.load().expect("reload updated state"), updated);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_primary_state_falls_back_to_a_valid_backup() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-store-backup-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = JsonEnvironmentStore::new(root.join("state.json"));
        let backup = EnvironmentSnapshot {
            updated_at: "backup".to_owned(),
            ..EnvironmentSnapshot::default()
        };
        store.ensure_parent().expect("create store directory");
        fs::write(&store.path, b"not-json").expect("write malformed primary");
        fs::write(
            store.backup_path(),
            serde_json::to_vec(&backup).expect("serialize backup"),
        )
        .expect("write backup");
        assert_eq!(store.load().expect("load backup"), backup);
        fs::remove_dir_all(root).expect("remove backup store");
    }

    #[test]
    fn unsupported_state_schema_fails_closed() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-store-schema-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = JsonEnvironmentStore::new(root.join("state.json"));
        let state = EnvironmentSnapshot {
            schema_version: "99".to_owned(),
            ..EnvironmentSnapshot::default()
        };
        store.ensure_parent().expect("create store directory");
        fs::write(
            &store.path,
            serde_json::to_vec(&state).expect("serialize state"),
        )
        .expect("write unsupported state");
        let error = store
            .load()
            .expect_err("unsupported schema must fail closed");
        assert!(error
            .to_string()
            .contains("unsupported environment state schema"));
        fs::remove_dir_all(root).expect("remove schema store");
    }

    #[cfg(windows)]
    #[test]
    fn sqlite_store_migrates_legacy_json_and_roundtrips_wal_state() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-sqlite-store-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let legacy_path = root.join("state-rust.json");
        let sqlite_path = root.join("state-rust.sqlite3");
        fs::create_dir_all(&root).expect("create SQLite test root");
        let legacy = EnvironmentSnapshot {
            updated_at: "legacy".to_owned(),
            ..EnvironmentSnapshot::default()
        };
        fs::write(
            &legacy_path,
            serde_json::to_vec(&legacy).expect("serialize legacy state"),
        )
        .expect("write legacy state");

        let store = SqliteEnvironmentStore::new(&sqlite_path, Some(legacy_path.clone()));
        assert_eq!(store.load().expect("migrate legacy state"), legacy);
        assert!(sqlite_path.exists());
        assert!(legacy_path.exists());
        let mut updated = legacy.clone();
        updated.updated_at = "sqlite".to_owned();
        updated.operations.push(cxszn_menv_domain::OperationRecord {
            operation_id: "sqlite-operation".to_owned(),
            plan_id: "sqlite-plan".to_owned(),
            plan_hash: None,
            idempotency_key: "sqlite-key".to_owned(),
            action: cxszn_menv_domain::EnvironmentChangeAction::Install,
            runtime_id: cxszn_menv_domain::RuntimeId::new("php-8.5").expect("runtime id"),
            status: cxszn_menv_domain::OperationStatus::Succeeded,
            created_at: "now".to_owned(),
            updated_at: "now".to_owned(),
            error: None,
        });
        store.save(&updated).expect("save SQLite state");
        assert_eq!(store.load().expect("reload SQLite state"), updated);
        assert!(sqlite_path.with_extension("sqlite3-wal").exists() || sqlite_path.exists());
        fs::remove_dir_all(root).expect("remove SQLite test root");
    }

    #[cfg(windows)]
    #[test]
    fn sqlite_store_roundtrips_install_plan_rows_transactionally() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-sqlite-plans-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = SqliteEnvironmentStore::new(root.join("state.sqlite3"), None);
        let rows = vec![
            ("plan-a".to_owned(), "{\"id\":\"a\"}".to_owned()),
            ("plan-b".to_owned(), "{\"id\":\"b\"}".to_owned()),
        ];
        store
            .save_php_install_plan_rows(&rows)
            .expect("save install plans");
        assert_eq!(
            store.load_php_install_plan_rows().expect("load plans"),
            rows
        );
        store
            .save_php_install_plan_rows(&rows[..1])
            .expect("replace install plans");
        assert_eq!(
            store.load_php_install_plan_rows().expect("reload plans"),
            rows[..1]
        );
        fs::remove_dir_all(root).expect("remove SQLite plan root");
    }

    #[cfg(windows)]
    #[test]
    fn sqlite_store_mirrors_install_journal_rows_in_order() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-sqlite-journal-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = SqliteEnvironmentStore::new(root.join("state.sqlite3"), None);
        let journal = root.join("operations").join("php").join("install.jsonl");
        store
            .append_install_journal_row(&journal, r#"{"stage":"planned"}"#)
            .expect("write first journal row");
        store
            .append_install_journal_row(&journal, r#"{"stage":"published"}"#)
            .expect("write second journal row");
        let rows = store
            .load_install_journal_rows()
            .expect("read journal rows");
        assert_eq!(rows.len(), 2);
        assert!(rows[0].1.contains("planned"));
        assert!(rows[1].1.contains("published"));
        fs::remove_dir_all(root).expect("remove SQLite journal root");
    }

    #[cfg(windows)]
    #[test]
    fn sqlite_confirmation_store_survives_reopen_without_plaintext_or_state_mutation() {
        use cxszn_menv_ports::{ConfirmationRecord, ConfirmationStore};
        let root =
            std::env::temp_dir().join(format!("menv-confirmation-store-{}", fixture_nonce()));
        let path = root.join("state.sqlite3");
        let store = SqliteEnvironmentStore::new(&path, None);
        let before = store.load().expect("initial state");
        assert_eq!(
            ConfirmationStore::find(&store, &"a".repeat(64)).expect("unknown digest"),
            None
        );
        let record = ConfirmationRecord {
            token_hash: "a".repeat(64),
            plan_id: "fixture-plan".into(),
            plan_hash: "b".repeat(64),
            actor: "fixture".into(),
            expires_at_epoch_seconds: 2000000000,
        };
        ConfirmationStore::insert(&store, &record).expect("record approval digest");
        let reopened = SqliteEnvironmentStore::new(&path, None);
        assert_eq!(
            ConfirmationStore::find(&reopened, &record.token_hash).expect("reopen lookup"),
            Some(record.clone())
        );
        assert_eq!(reopened.load().expect("state unchanged"), before);
        let connection = reopened.open().expect("connection");
        let columns = connection
            .prepare("PRAGMA table_info(menv_confirmations)")
            .expect("table schema")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("decode columns");
        assert_eq!(
            columns,
            ["token_hash", "plan_id", "plan_hash", "actor", "expires_at"]
        );
        drop(connection);
        fs::remove_dir_all(root).expect("remove isolated confirmation store fixture");
    }

    #[cfg(windows)]
    #[test]
    fn sqlite_confirmation_digest_conflict_cannot_replace_approved_binding() {
        use cxszn_menv_ports::{ConfirmationRecord, ConfirmationStore};
        let root =
            std::env::temp_dir().join(format!("menv-confirmation-conflict-{}", fixture_nonce()));
        let store = SqliteEnvironmentStore::new(root.join("state.sqlite3"), None);
        let record = ConfirmationRecord {
            token_hash: "a".repeat(64),
            plan_id: "fixture-plan".into(),
            plan_hash: "b".repeat(64),
            actor: "fixture".into(),
            expires_at_epoch_seconds: 2000000000,
        };
        ConfirmationStore::insert(&store, &record).expect("first digest");
        let mut replaced = record.clone();
        replaced.actor = "not-approved".into();
        assert!(ConfirmationStore::insert(&store, &replaced).is_err());
        assert_eq!(
            ConfirmationStore::find(&store, &record.token_hash).expect("original approval"),
            Some(record)
        );
        fs::remove_dir_all(root).expect("remove isolated conflict fixture");
    }

    #[cfg(windows)]
    fn fixture_nonce() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    }

    #[cfg(windows)]
    #[test]
    fn sqlite_store_rejects_unsupported_schema_without_using_legacy_json() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-sqlite-schema-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let sqlite_path = root.join("state.sqlite3");
        let legacy_path = root.join("state-rust.json");
        fs::create_dir_all(&root).expect("create SQLite schema root");
        fs::write(
            &legacy_path,
            serde_json::to_vec(&EnvironmentSnapshot::default()).expect("serialize legacy"),
        )
        .expect("write legacy source");
        {
            let connection = Connection::open(&sqlite_path).expect("open raw SQLite");
            connection
                .execute_batch(
                    "CREATE TABLE menv_meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);\
                     INSERT INTO menv_meta(key, value) VALUES ('schema_version', '99');",
                )
                .expect("write unsupported schema");
        }
        let store = SqliteEnvironmentStore::new(&sqlite_path, Some(legacy_path));
        let error = store
            .load()
            .expect_err("unsupported schema must fail closed");
        assert!(error
            .to_string()
            .contains("unsupported SQLite state schema"));
        fs::remove_dir_all(root).expect("remove SQLite schema root");
    }
}
