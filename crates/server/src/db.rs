use anyhow::{anyhow, Context, Result};
use common::sealed;
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
        );",
    )
    .context("failed to create schema")?;
    Ok(())
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
