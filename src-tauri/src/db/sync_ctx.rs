//! The sync "applying" flag (ARCHITECTURE §4.1, §6.2). While it is 1, SQL **policy** triggers stand
//! down so pulled rows — already validated on their origin device and by the server — can be applied
//! parents-first even when they arrive in an order local rules would reject. Structural and derived
//! triggers always run.
//!
//! Only the sync pull (Phase 3) may call these, and only inside its own page transaction obtained from
//! `Db::write()`. Because the flag is a table row, a rolled-back page restores it to 0 automatically.

use sqlx::SqliteConnection;

use crate::error::AppResult;

pub async fn begin_applying(conn: &mut SqliteConnection) -> AppResult<()> {
    sqlx::query("UPDATE sync_ctx SET applying = 1 WHERE id = 1").execute(&mut *conn).await?;
    Ok(())
}

pub async fn end_applying(conn: &mut SqliteConnection) -> AppResult<()> {
    sqlx::query("UPDATE sync_ctx SET applying = 0 WHERE id = 1").execute(&mut *conn).await?;
    Ok(())
}

pub async fn is_applying(conn: &mut SqliteConnection) -> AppResult<bool> {
    let v: i64 = sqlx::query_scalar("SELECT applying FROM sync_ctx WHERE id = 1").fetch_one(&mut *conn).await?;
    Ok(v == 1)
}
