//! Archivio SQLite del nodo. Contiene solo dati opachi o cifrati (CLAUDE.md, regola 2).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

pub type Result<T> = rusqlite::Result<T>;

pub struct Store {
    conn: Connection,
}

pub struct StoredMessage {
    pub id: String,
    pub blob: Vec<u8>,
    pub received_at: u64,
}

pub struct StoredBoardEntry {
    pub owner: String,
    pub blob: String,
    pub expires_at: u64,
    pub sig: String,
}

pub struct PushTarget {
    pub kind: String,
    pub target: String,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS files (
    id TEXT PRIMARY KEY,
    inbox TEXT NOT NULL,
    data BLOB NOT NULL,
    received_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS files_inbox ON files (inbox);
CREATE TABLE IF NOT EXISTS shared_files (
    id TEXT PRIMARY KEY,
    inbox TEXT NOT NULL,
    data BLOB NOT NULL,
    received_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS shared_inbox ON shared_files (inbox);
CREATE TABLE IF NOT EXISTS phone_pending (
    inbox TEXT PRIMARY KEY,
    phone_hash TEXT NOT NULL,
    code TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL,
    last_sent_at INTEGER NOT NULL,
    day INTEGER NOT NULL,
    sent_today INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS phone_dir (
    phone_hash TEXT PRIMARY KEY,
    inbox TEXT NOT NULL,
    entry TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS inboxes (
    inbox TEXT PRIMARY KEY,
    owner TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS revoked (
    permit_id TEXT PRIMARY KEY,
    inbox TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS push_targets (
    inbox TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    target TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    inbox TEXT NOT NULL,
    blob BLOB NOT NULL,
    received_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_inbox ON messages (inbox, received_at);
CREATE TABLE IF NOT EXISTS keypackages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    inbox TEXT NOT NULL,
    kp BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS keypackages_inbox ON keypackages (inbox, id);
CREATE TABLE IF NOT EXISTS names (
    name TEXT PRIMARY KEY,
    identity TEXT NOT NULL,
    record TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS board (
    loc TEXT NOT NULL,
    owner TEXT NOT NULL,
    blob TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    sig TEXT NOT NULL,
    PRIMARY KEY (loc, owner)
);
";

fn to_i64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn to_u64(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        // Migrazione: scadenza dei messaggi effimeri (SPEC §6), assente negli archivi vecchi.
        let has_expiry = conn
            .prepare("SELECT 1 FROM pragma_table_info('messages') WHERE name = 'expires_at'")?
            .exists([])?;
        if !has_expiry {
            conn.execute("ALTER TABLE messages ADD COLUMN expires_at INTEGER", [])?;
        }
        Ok(Self { conn })
    }

    // --- caselle ---

    pub fn create_inbox(&self, inbox: &str, owner: &str, now: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO inboxes (inbox, owner, created_at) VALUES (?1, ?2, ?3)",
            params![inbox, owner, to_i64(now)],
        )?;
        Ok(())
    }

    pub fn inbox_owner(&self, inbox: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT owner FROM inboxes WHERE inbox = ?1", [inbox], |r| {
                r.get(0)
            })
            .optional()
    }

    // --- numeri verificati (SPEC §13) ---

    pub fn phone_pending(&self, inbox: &str) -> Result<Option<PhonePending>> {
        self.conn
            .query_row(
                "SELECT phone_hash, code, expires_at, attempts, last_sent_at, day, sent_today
                 FROM phone_pending WHERE inbox = ?1",
                [inbox],
                |r| {
                    Ok(PhonePending {
                        phone_hash: r.get(0)?,
                        code: r.get(1)?,
                        expires_at: to_u64(r.get(2)?),
                        attempts: to_u64(r.get(3)?),
                        last_sent_at: to_u64(r.get(4)?),
                        day: to_u64(r.get(5)?),
                        sent_today: to_u64(r.get(6)?),
                    })
                },
            )
            .optional()
    }

    pub fn put_phone_pending(&self, inbox: &str, p: &PhonePending) -> Result<()> {
        self.conn.execute(
            "INSERT INTO phone_pending
               (inbox, phone_hash, code, expires_at, attempts, last_sent_at, day, sent_today)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(inbox) DO UPDATE SET phone_hash = excluded.phone_hash,
               code = excluded.code, expires_at = excluded.expires_at,
               attempts = excluded.attempts, last_sent_at = excluded.last_sent_at,
               day = excluded.day, sent_today = excluded.sent_today",
            params![
                inbox,
                p.phone_hash,
                p.code,
                to_i64(p.expires_at),
                to_i64(p.attempts),
                to_i64(p.last_sent_at),
                to_i64(p.day),
                to_i64(p.sent_today)
            ],
        )?;
        Ok(())
    }

    pub fn phone_attempt(&self, inbox: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE phone_pending SET attempts = attempts + 1 WHERE inbox = ?1",
            [inbox],
        )?;
        Ok(())
    }

    /// Verifica riuscita: il codice non vale più (restano i contatori per i limiti).
    pub fn phone_done(&self, inbox: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE phone_pending SET code = '', expires_at = 0 WHERE inbox = ?1",
            [inbox],
        )?;
        Ok(())
    }

    /// Pubblica (o sposta a questa casella) un numero verificato.
    pub fn put_phone_dir(&self, phone_hash: &str, inbox: &str, entry: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO phone_dir (phone_hash, inbox, entry) VALUES (?1, ?2, ?3)
             ON CONFLICT(phone_hash) DO UPDATE SET inbox = excluded.inbox, entry = excluded.entry",
            params![phone_hash, inbox, entry],
        )?;
        Ok(())
    }

    pub fn phone_dir(&self, phone_hash: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT entry FROM phone_dir WHERE phone_hash = ?1",
                [phone_hash],
                |r| r.get(0),
            )
            .optional()
    }

    pub fn delete_phone_dir(&self, inbox: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM phone_dir WHERE inbox = ?1", [inbox])?;
        Ok(())
    }

    // --- permessi ---

    pub fn revoke(&self, inbox: &str, permit_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO revoked (permit_id, inbox) VALUES (?1, ?2)",
            params![permit_id, inbox],
        )?;
        Ok(())
    }

    pub fn is_revoked(&self, permit_id: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT 1 FROM revoked WHERE permit_id = ?1",
                [permit_id],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
    }

    // --- push ---

    pub fn set_push(&self, inbox: &str, kind: &str, target: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO push_targets (inbox, kind, target) VALUES (?1, ?2, ?3)
             ON CONFLICT (inbox) DO UPDATE SET kind = excluded.kind, target = excluded.target",
            params![inbox, kind, target],
        )?;
        Ok(())
    }

    pub fn clear_push(&self, inbox: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM push_targets WHERE inbox = ?1", [inbox])?;
        Ok(())
    }

    pub fn push_target(&self, inbox: &str) -> Result<Option<PushTarget>> {
        self.conn
            .query_row(
                "SELECT kind, target FROM push_targets WHERE inbox = ?1",
                [inbox],
                |r| {
                    Ok(PushTarget {
                        kind: r.get(0)?,
                        target: r.get(1)?,
                    })
                },
            )
            .optional()
    }

    // --- messaggi ---

    /// `expires_at`: solo per i messaggi effimeri, che spariscono anche se nessuno li ritira.
    pub fn put_message(
        &self,
        id: &str,
        inbox: &str,
        blob: &[u8],
        now: u64,
        expires_at: Option<u64>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages (id, inbox, blob, received_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, inbox, blob, to_i64(now), expires_at.map(to_i64)],
        )?;
        Ok(())
    }

    pub fn count_messages(&self, inbox: &str) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE inbox = ?1",
                [inbox],
                |r| r.get::<_, i64>(0),
            )
            .map(to_u64)
    }

    pub fn messages(&self, inbox: &str, limit: u32, now: u64) -> Result<Vec<StoredMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, blob, received_at FROM messages
             WHERE inbox = ?1 AND (expires_at IS NULL OR expires_at > ?3)
             ORDER BY received_at, rowid LIMIT ?2",
        )?;
        stmt.query_map(params![inbox, limit, to_i64(now)], |r| {
            Ok(StoredMessage {
                id: r.get(0)?,
                blob: r.get(1)?,
                received_at: to_u64(r.get(2)?),
            })
        })?
        .collect()
    }

    pub fn ack(&self, inbox: &str, ids: &[String]) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("DELETE FROM messages WHERE inbox = ?1 AND id = ?2")?;
        for id in ids {
            stmt.execute(params![inbox, id])?;
        }
        Ok(())
    }

    // --- key package ---

    pub fn add_keypackages(&self, inbox: &str, items: &[Vec<u8>]) -> Result<()> {
        let mut stmt = self
            .conn
            .prepare("INSERT INTO keypackages (inbox, kp) VALUES (?1, ?2)")?;
        for kp in items {
            stmt.execute(params![inbox, kp])?;
        }
        Ok(())
    }

    pub fn count_keypackages(&self, inbox: &str) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM keypackages WHERE inbox = ?1",
                [inbox],
                |r| r.get::<_, i64>(0),
            )
            .map(to_u64)
    }

    /// Prende il key package più vecchio; l'ultimo rimasto non si cancella (SPEC §11).
    pub fn claim_keypackage(&self, inbox: &str) -> Result<Option<Vec<u8>>> {
        let row: Option<(i64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT id, kp FROM keypackages WHERE inbox = ?1 ORDER BY id LIMIT 1",
                [inbox],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((id, kp)) = row else {
            return Ok(None);
        };
        if self.count_keypackages(inbox)? > 1 {
            self.conn
                .execute("DELETE FROM keypackages WHERE id = ?1", [id])?;
        }
        Ok(Some(kp))
    }

    // --- nomi ---

    pub fn name(&self, name: &str) -> Result<Option<(String, String)>> {
        self.conn
            .query_row(
                "SELECT identity, record FROM names WHERE name = ?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
    }

    pub fn put_name(&self, name: &str, identity: &str, record: &str, now: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO names (name, identity, record, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (name) DO UPDATE SET record = excluded.record,
                 updated_at = excluded.updated_at",
            params![name, identity, record, to_i64(now)],
        )?;
        Ok(())
    }

    pub fn names_of(&self, identity: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM names WHERE identity = ?1")?;
        stmt.query_map([identity], |r| r.get(0))?.collect()
    }

    pub fn delete_name(&self, name: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM names WHERE name = ?1", [name])?;
        Ok(())
    }

    // --- bacheca ---

    pub fn board(&self, loc: &str, now: u64) -> Result<Vec<StoredBoardEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT owner, blob, expires_at, sig FROM board WHERE loc = ?1 AND expires_at > ?2",
        )?;
        stmt.query_map(params![loc, to_i64(now)], |r| {
            Ok(StoredBoardEntry {
                owner: r.get(0)?,
                blob: r.get(1)?,
                expires_at: to_u64(r.get(2)?),
                sig: r.get(3)?,
            })
        })?
        .collect()
    }

    pub fn has_board_entry(&self, loc: &str, owner: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT 1 FROM board WHERE loc = ?1 AND owner = ?2",
                [loc, owner],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
    }

    pub fn put_board(&self, loc: &str, e: &StoredBoardEntry) -> Result<()> {
        self.conn.execute(
            "INSERT INTO board (loc, owner, blob, expires_at, sig) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (loc, owner) DO UPDATE SET blob = excluded.blob,
                 expires_at = excluded.expires_at, sig = excluded.sig",
            params![loc, e.owner, e.blob, to_i64(e.expires_at), e.sig],
        )?;
        Ok(())
    }

    // --- file (SPEC §6.2) ---

    pub fn put_file(&self, id: &str, inbox: &str, data: &[u8], now: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO files (id, inbox, data, received_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, inbox, data, to_i64(now)],
        )?;
        Ok(())
    }

    pub fn file(&self, inbox: &str, id: &str) -> Result<Option<Vec<u8>>> {
        self.conn
            .query_row(
                "SELECT data FROM files WHERE inbox = ?1 AND id = ?2",
                [inbox, id],
                |r| r.get(0),
            )
            .optional()
    }

    pub fn delete_file(&self, inbox: &str, id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM files WHERE inbox = ?1 AND id = ?2",
            [inbox, id],
        )?;
        Ok(())
    }

    /// Spazio occupato dai file di una casella.
    pub fn file_bytes(&self, inbox: &str) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM files WHERE inbox = ?1",
                [inbox],
                |r| r.get::<_, i64>(0),
            )
            .map(to_u64)
    }

    // --- file condivisi (SPEC §6.2, gruppi) ---

    pub fn put_shared(&self, id: &str, inbox: &str, data: &[u8], now: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO shared_files (id, inbox, data, received_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, inbox, data, to_i64(now)],
        )?;
        Ok(())
    }

    pub fn shared(&self, id: &str) -> Result<Option<Vec<u8>>> {
        self.conn
            .query_row("SELECT data FROM shared_files WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()
    }

    pub fn shared_bytes(&self, inbox: &str) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM shared_files WHERE inbox = ?1",
                [inbox],
                |r| r.get::<_, i64>(0),
            )
            .map(to_u64)
    }

    // --- pulizia ---

    pub fn purge(&self, now: u64, message_ttl: u64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM messages WHERE received_at < ?1",
            [to_i64(now.saturating_sub(message_ttl))],
        )?;
        self.conn.execute(
            "DELETE FROM messages WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            [to_i64(now)],
        )?;
        self.conn.execute(
            "DELETE FROM files WHERE received_at < ?1",
            [to_i64(now.saturating_sub(message_ttl))],
        )?;
        self.conn.execute(
            "DELETE FROM shared_files WHERE received_at < ?1",
            [to_i64(now.saturating_sub(message_ttl))],
        )?;
        self.conn
            .execute("DELETE FROM board WHERE expires_at <= ?1", [to_i64(now)])?;
        Ok(())
    }
}

/// Verifica del numero in corso per una casella (SPEC §13.2).
#[derive(Debug, Clone)]
pub struct PhonePending {
    pub phone_hash: String,
    pub code: String,
    pub expires_at: u64,
    pub attempts: u64,
    pub last_sent_at: u64,
    pub day: u64,
    pub sent_today: u64,
}
