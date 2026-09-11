use anyhow::{anyhow, Context, Result};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use common::sealed;
use rand::rngs::OsRng;
use rusqlite::{params, Connection, OptionalExtension};

pub struct Meta {
    pub master_key: [u8; sealed::KEY_LEN],
    pub ca_cert_pem: String,
    pub ca_key_pem: String,
}

pub struct BuildRecord {
    pub id: String,
    pub created_at: String,
    pub status: String,
    pub artifact_path: String,
    pub pub_armored: String,
    pub priv_armored: String,
    pub passphrase: String,
    pub client_cn: String,
}

pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("failed to open db {path}"))?;
    conn.pragma_update(None, "journal_mode", "WAL").ok();
    Ok(conn)
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            kdf_salt BLOB NOT NULL,
            ca_cert_pem TEXT NOT NULL,
            ca_key_sealed BLOB NOT NULL,
            ca_key_nonce BLOB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS builds (
            id TEXT PRIMARY KEY,
            created_at TEXT NOT NULL,
            status TEXT NOT NULL,
            artifact_path TEXT NOT NULL,
            pub_armored TEXT NOT NULL,
            priv_sealed BLOB NOT NULL,
            priv_nonce BLOB NOT NULL,
            passphrase_sealed BLOB NOT NULL,
            passphrase_nonce BLOB NOT NULL,
            client_cn TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS operator_auth (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            password_hash TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS users (
            username TEXT PRIMARY KEY,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1
        );",
    )
    .context("failed to create schema")?;
    Ok(())
}

pub fn operator_configured(conn: &Connection) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM operator_auth WHERE id = 1)",
        [],
        |row| row.get(0),
    )
    .context("failed to query operator authentication")
}

pub fn set_operator_password(conn: &Connection, password: &str) -> Result<()> {
    set_user_password(conn, "operator", password)?;
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("failed to hash operator password: {e}"))?
        .to_string();
    conn.execute(
        "INSERT INTO operator_auth (id, password_hash) VALUES (1, ?1)
         ON CONFLICT(id) DO UPDATE SET password_hash = excluded.password_hash",
        params![password_hash],
    )
    .context("failed to save operator authentication")?;
    Ok(())
}

pub fn any_user_configured(conn: &Connection) -> Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM users)", [], |row| row.get(0))
        .context("failed to query users")
}

pub fn set_user_password(conn: &Connection, username: &str, password: &str) -> Result<()> {
    if username.trim().is_empty() || username.contains(char::is_whitespace) {
        anyhow::bail!("username cannot be empty or contain whitespace");
    }
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("failed to hash user password: {e}"))?
        .to_string();
    conn.execute(
        "INSERT INTO users (username, password_hash, created_at, enabled) VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(username) DO UPDATE SET password_hash = excluded.password_hash, enabled = 1",
        params![username, password_hash, now_string()],
    )
    .context("failed to save user")?;
    Ok(())
}

pub fn verify_user_password(conn: &Connection, username: &str, password: &str) -> Result<bool> {
    let hash: Option<String> = conn
        .query_row(
            "SELECT password_hash FROM users WHERE username = ?1 AND enabled = 1",
            params![username],
            |row| row.get(0),
        )
        .optional()
        .context("failed to query user")?;
    let Some(hash) = hash else {
        return Ok(false);
    };
    let parsed = PasswordHash::new(&hash)
        .map_err(|e| anyhow!("stored user password hash is invalid: {e}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn list_users(conn: &Connection) -> Result<Vec<(String, String, bool)>> {
    let mut stmt = conn
        .prepare("SELECT username, created_at, enabled FROM users ORDER BY username")
        .context("failed to prepare user listing")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? != 0))
        })
        .context("failed to list users")?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to read user listing")
}

fn now_string() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}

pub fn verify_operator_password(conn: &Connection, password: &str) -> Result<bool> {
    let hash: Option<String> = conn
        .query_row(
            "SELECT password_hash FROM operator_auth WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .context("failed to query operator authentication")?;
    let Some(hash) = hash else {
        return Ok(false);
    };
    let parsed = PasswordHash::new(&hash)
        .map_err(|e| anyhow!("stored operator password hash is invalid: {e}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

pub fn list_builds(conn: &Connection, limit: usize) -> Result<Vec<(String, String, String, String)>> {
    let mut stmt = conn
        .prepare("SELECT id, created_at, status, artifact_path FROM builds ORDER BY created_at DESC LIMIT ?1")
        .context("failed to prepare build listing")?;
    let rows = stmt
        .query_map(params![limit as i64], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        })
        .context("failed to list builds")?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to read build listing")
}

pub fn load_or_create_meta(
    conn: &Connection,
    passphrase: &str,
    make_ca: impl FnOnce() -> Result<(String, String)>,
) -> Result<Meta> {
    let existing: Option<(Vec<u8>, String, Vec<u8>, Vec<u8>)> = conn
        .query_row(
            "SELECT kdf_salt, ca_cert_pem, ca_key_sealed, ca_key_nonce FROM meta WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            },
        )
        .optional()
        .context("failed to query meta")?;

    if let Some((salt, ca_cert_pem, ca_key_sealed, ca_key_nonce)) = existing {
        let master_key = sealed::derive_master_key(passphrase, &salt)?;
        let ca_key_pem = sealed::open_str(&master_key, &ca_key_nonce, &ca_key_sealed)
            .context("failed to unseal ca key (wrong master passphrase?)")?;
        return Ok(Meta {
            master_key,
            ca_cert_pem,
            ca_key_pem,
        });
    }

    let salt = sealed::random_salt();
    let master_key = sealed::derive_master_key(passphrase, &salt)?;
    let (ca_cert_pem, ca_key_pem) = make_ca()?;
    let (ca_key_nonce, ca_key_sealed) = sealed::seal_str(&master_key, &ca_key_pem)?;
    conn.execute(
        "INSERT INTO meta (id, kdf_salt, ca_cert_pem, ca_key_sealed, ca_key_nonce)
         VALUES (1, ?1, ?2, ?3, ?4)",
        params![salt.to_vec(), ca_cert_pem, ca_key_sealed, ca_key_nonce],
    )
    .context("failed to insert meta")?;

    Ok(Meta {
        master_key,
        ca_cert_pem,
        ca_key_pem,
    })
}

pub fn insert_build(
    conn: &Connection,
    key: &[u8; sealed::KEY_LEN],
    rec: &BuildRecord,
) -> Result<()> {
    let (priv_nonce, priv_sealed) = sealed::seal_str(key, &rec.priv_armored)?;
    let (pass_nonce, pass_sealed) = sealed::seal_str(key, &rec.passphrase)?;
    conn.execute(
        "INSERT INTO builds (
            id, created_at, status, artifact_path, pub_armored,
            priv_sealed, priv_nonce, passphrase_sealed, passphrase_nonce, client_cn
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            rec.id,
            rec.created_at,
            rec.status,
            rec.artifact_path,
            rec.pub_armored,
            priv_sealed,
            priv_nonce,
            pass_sealed,
            pass_nonce,
            rec.client_cn,
        ],
    )
    .context("failed to insert build")?;
    Ok(())
}

pub fn get_build_secret(
    conn: &Connection,
    key: &[u8; sealed::KEY_LEN],
    build_id: &str,
) -> Result<Option<(String, String, String)>> {
    let row: Option<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, String)> = conn
        .query_row(
            "SELECT priv_sealed, priv_nonce, passphrase_sealed, passphrase_nonce, client_cn
             FROM builds WHERE id = ?1",
            params![build_id],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()
        .context("failed to query build")?;

    let Some((priv_sealed, priv_nonce, pass_sealed, pass_nonce, client_cn)) = row else {
        return Ok(None);
    };

    let priv_armored = sealed::open_str(key, &priv_nonce, &priv_sealed)?;
    let passphrase = sealed::open_str(key, &pass_nonce, &pass_sealed)?;
    Ok(Some((priv_armored, passphrase, client_cn)))
}

pub fn get_build_artifact(conn: &Connection, build_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT artifact_path FROM builds WHERE id = ?1",
        params![build_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .context("failed to query artifact path")
    .map_err(|e| anyhow!(e))
}
