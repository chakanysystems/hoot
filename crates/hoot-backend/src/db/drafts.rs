use anyhow::Result;

use super::Db;

#[derive(Clone, Debug)]
pub struct Draft {
    pub id: i64,
    pub subject: String,
    pub to_field: String,
    pub content: String,
    pub parent_events: Vec<String>,
    pub selected_account: Option<String>,
    pub selected_nip05: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Db {
    pub fn save_draft(
        &self,
        subject: &str,
        to_field: &str,
        content: &str,
        parent_events: &[String],
        selected_account: Option<&str>,
        selected_nip05: Option<&str>,
    ) -> Result<i64> {
        let parent_events_json = serde_json::to_string(parent_events)?;
        self.connection.execute(
            "INSERT INTO drafts (subject, to_field, content, parent_events, selected_account, selected_nip05)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                subject,
                to_field,
                content,
                &parent_events_json,
                selected_account,
                selected_nip05,
            ),
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub fn update_draft(
        &self,
        id: i64,
        subject: &str,
        to_field: &str,
        content: &str,
        parent_events: &[String],
        selected_account: Option<&str>,
        selected_nip05: Option<&str>,
    ) -> Result<()> {
        let parent_events_json = serde_json::to_string(parent_events)?;
        self.connection.execute(
            "UPDATE drafts SET subject = ?1, to_field = ?2, content = ?3,
             parent_events = ?4, selected_account = ?5, selected_nip05 = ?6, updated_at = unixepoch()
             WHERE id = ?7",
            (
                subject,
                to_field,
                content,
                &parent_events_json,
                selected_account,
                selected_nip05,
                id,
            ),
        )?;
        Ok(())
    }

    pub fn get_drafts(&self) -> Result<Vec<Draft>> {
        let mut stmt = self.connection.prepare(
            "SELECT id, subject, to_field, content, parent_events, selected_account, selected_nip05, created_at, updated_at
             FROM drafts ORDER BY updated_at DESC",
        )?;

        let drafts_iter = stmt.query_map([], |row| {
            let parent_events_json: String = row.get(4)?;
            let parent_events: Vec<String> =
                serde_json::from_str(&parent_events_json).unwrap_or_default();

            Ok(Draft {
                id: row.get(0)?,
                subject: row.get(1)?,
                to_field: row.get(2)?,
                content: row.get(3)?,
                parent_events,
                selected_account: row.get(5)?,
                selected_nip05: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;

        let drafts = drafts_iter.collect::<Result<Vec<Draft>, rusqlite::Error>>()?;
        Ok(drafts)
    }

    pub fn delete_draft(&self, id: i64) -> Result<()> {
        self.connection
            .execute("DELETE FROM drafts WHERE id = ?1", (id,))?;
        Ok(())
    }

    pub fn get_draft_count(&self) -> Result<i64> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM drafts", [], |row| row.get(0))?;
        Ok(count)
    }
}
