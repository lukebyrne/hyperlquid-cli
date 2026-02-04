use std::fs;

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::paths;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Account {
    pub id: i64,
    pub alias: String,
    #[serde(rename = "userAddress")]
    pub user_address: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub source: String,
    #[serde(rename = "apiWalletPrivateKey")]
    pub api_wallet_private_key: Option<String>,
    #[serde(rename = "apiWalletPublicKey")]
    pub api_wallet_public_key: Option<String>,
    #[serde(rename = "isDefault")]
    pub is_default: bool,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

#[derive(Clone, Debug)]
pub struct CreateAccountInput {
    pub alias: String,
    pub user_address: String,
    pub account_type: String, // "readonly" | "api_wallet"
    pub source: Option<String>,
    pub api_wallet_private_key: Option<String>,
    pub api_wallet_public_key: Option<String>,
    pub set_as_default: bool,
}

fn connect() -> Result<Connection> {
    let hl_dir = paths::hl_dir()?;
    fs::create_dir_all(&hl_dir)
        .with_context(|| format!("Failed to create dir: {}", hl_dir.display()))?;

    let db_path = paths::db_path()?;
    let conn = Connection::open(&db_path)
        .with_context(|| format!("Failed to open db: {}", db_path.display()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    run_migrations(&conn)?;
    Ok(conn)
}

fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS migrations (
          id INTEGER PRIMARY KEY,
          name TEXT NOT NULL UNIQUE,
          applied_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );
      "#,
    )?;

    let applied: Vec<String> = conn
        .prepare("SELECT name FROM migrations")?
        .query_map([], |row| row.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let apply_migration = |name: &str, sql: &str| -> Result<()> {
        if applied.iter().any(|n| n == name) {
            return Ok(());
        }
        conn.execute_batch(sql)?;
        conn.execute("INSERT INTO migrations (name) VALUES (?)", params![name])?;
        Ok(())
    };

    apply_migration(
        "001_create_accounts",
        r#"
        CREATE TABLE IF NOT EXISTS accounts (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          alias TEXT NOT NULL UNIQUE,
          user_address TEXT NOT NULL,
          type TEXT NOT NULL CHECK (type IN ('readonly', 'api_wallet')),
          source TEXT NOT NULL DEFAULT 'cli_import',
          api_wallet_private_key TEXT,
          api_wallet_public_key TEXT,
          is_default INTEGER NOT NULL DEFAULT 0,
          created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
          updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );

        CREATE INDEX IF NOT EXISTS idx_accounts_is_default ON accounts(is_default);
        CREATE INDEX IF NOT EXISTS idx_accounts_user_address ON accounts(user_address);
      "#,
    )?;

    Ok(())
}

fn row_to_account(row: &Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get("id")?,
        alias: row.get("alias")?,
        user_address: row.get("user_address")?,
        account_type: row.get("type")?,
        source: row.get("source")?,
        api_wallet_private_key: row.get("api_wallet_private_key")?,
        api_wallet_public_key: row.get("api_wallet_public_key")?,
        is_default: row.get::<_, i64>("is_default")? == 1,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn create_account(input: CreateAccountInput) -> Result<Account> {
    let mut conn = connect()?;
    let tx = conn.transaction()?;

    let account_count: i64 = tx.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
    let should_be_default = account_count == 0 || input.set_as_default;

    if should_be_default {
        tx.execute(
            "UPDATE accounts SET is_default = 0 WHERE is_default = 1",
            [],
        )?;
    }

    tx.execute(
        r#"
        INSERT INTO accounts (
          alias,
          user_address,
          type,
          source,
          api_wallet_private_key,
          api_wallet_public_key,
          is_default
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
      "#,
        params![
            input.alias,
            input.user_address,
            input.account_type,
            input.source.unwrap_or_else(|| "cli_import".to_string()),
            input.api_wallet_private_key,
            input.api_wallet_public_key,
            if should_be_default { 1 } else { 0 },
        ],
    )?;

    let id = tx.last_insert_rowid();
    let account = tx.query_row(
        "SELECT * FROM accounts WHERE id = ?",
        params![id],
        row_to_account,
    )?;
    tx.commit()?;
    Ok(account)
}

pub fn get_account_by_id(id: i64) -> Result<Option<Account>> {
    let conn = connect()?;
    let account = conn
        .query_row(
            "SELECT * FROM accounts WHERE id = ?",
            params![id],
            row_to_account,
        )
        .optional()?;
    Ok(account)
}

pub fn get_account_by_alias(alias: &str) -> Result<Option<Account>> {
    let conn = connect()?;
    let account = conn
        .query_row(
            "SELECT * FROM accounts WHERE alias = ?",
            params![alias],
            row_to_account,
        )
        .optional()?;
    Ok(account)
}

pub fn get_default_account() -> Result<Option<Account>> {
    let conn = connect()?;
    let account = conn
        .query_row(
            "SELECT * FROM accounts WHERE is_default = 1",
            [],
            row_to_account,
        )
        .optional()?;
    Ok(account)
}

pub fn get_all_accounts() -> Result<Vec<Account>> {
    let conn = connect()?;
    let mut stmt =
        conn.prepare("SELECT * FROM accounts ORDER BY is_default DESC, created_at ASC")?;
    let accounts = stmt
        .query_map([], row_to_account)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(accounts)
}

pub fn set_default_account(alias: &str) -> Result<Account> {
    let mut conn = connect()?;
    let tx = conn.transaction()?;

    let existing: Option<Account> = tx
        .query_row(
            "SELECT * FROM accounts WHERE alias = ?",
            params![alias],
            row_to_account,
        )
        .optional()?;

    let Some(existing) = existing else {
        return Err(anyhow!("Account with alias \"{alias}\" not found"));
    };

    if existing.is_default {
        tx.commit()?;
        return Ok(existing);
    }

    tx.execute(
        "UPDATE accounts SET is_default = 0 WHERE is_default = 1",
        [],
    )?;
    tx.execute(
        "UPDATE accounts SET is_default = 1, updated_at = strftime('%s', 'now') WHERE alias = ?",
        params![alias],
    )?;

    let updated = tx.query_row(
        "SELECT * FROM accounts WHERE alias = ?",
        params![alias],
        row_to_account,
    )?;
    tx.commit()?;

    Ok(updated)
}

pub fn delete_account(alias: &str) -> Result<bool> {
    let mut conn = connect()?;
    let tx = conn.transaction()?;

    let existing: Option<Account> = tx
        .query_row(
            "SELECT * FROM accounts WHERE alias = ?",
            params![alias],
            row_to_account,
        )
        .optional()?;

    let Some(existing) = existing else {
        tx.commit()?;
        return Ok(false);
    };

    let was_default = existing.is_default;
    tx.execute("DELETE FROM accounts WHERE alias = ?", params![alias])?;

    if was_default {
        // set first remaining as default (created_at ASC)
        let first_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM accounts ORDER BY created_at ASC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = first_id {
            tx.execute(
                "UPDATE accounts SET is_default = 1 WHERE id = ?",
                params![id],
            )?;
        }
    }

    tx.commit()?;
    Ok(true)
}

pub fn is_alias_taken(alias: &str) -> Result<bool> {
    let conn = connect()?;
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM accounts WHERE alias = ?",
            params![alias],
            |row| row.get(0),
        )
        .optional()?;
    Ok(exists.is_some())
}

pub fn get_account_count() -> Result<i64> {
    let conn = connect()?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?;
    Ok(count)
}
