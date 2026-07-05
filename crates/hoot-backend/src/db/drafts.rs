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

pub struct DraftUpdate<'a> {
    pub id: i64,
    pub subject: &'a str,
    pub to_field: &'a str,
    pub content: &'a str,
    pub parent_events: &'a [String],
    pub selected_account: Option<&'a str>,
    pub selected_nip05: Option<&'a str>,
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

    pub fn update_draft(&self, draft: DraftUpdate<'_>) -> Result<()> {
        let parent_events_json = serde_json::to_string(draft.parent_events)?;
        self.connection.execute(
            "UPDATE drafts SET subject = ?1, to_field = ?2, content = ?3,
             parent_events = ?4, selected_account = ?5, selected_nip05 = ?6, updated_at = unixepoch()
             WHERE id = ?7",
            (
                draft.subject,
                draft.to_field,
                draft.content,
                &parent_events_json,
                draft.selected_account,
                draft.selected_nip05,
                draft.id,
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

    #[cfg(test)]
    pub fn get_draft_count(&self) -> Result<i64> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM drafts", [], |row| row.get(0))?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_lifecycle_preserves_fields_and_orders_by_updated_at() -> Result<()> {
        let db = Db::new_in_memory()?;
        let first_parents = vec!["parent-root".to_string(), "parent-reply".to_string()];
        let second_parents = vec!["older-thread".to_string()];

        let first_id = db.save_draft(
            "Original subject",
            "alice@example.com, bob@example.com",
            "Original body",
            &first_parents,
            Some("account-a"),
            Some("alice@example.com"),
        )?;
        let second_id = db.save_draft(
            "Second subject",
            "carol@example.com",
            "Second body",
            &second_parents,
            None,
            None,
        )?;
        assert_eq!(db.get_draft_count()?, 2);

        let replacement_parents = vec!["new-root".to_string(), "new-reply".to_string()];
        db.update_draft(DraftUpdate {
            id: first_id,
            subject: "Updated subject",
            to_field: "dave@example.com",
            content: "Updated body",
            parent_events: &replacement_parents,
            selected_account: Some("account-b"),
            selected_nip05: Some("dave@example.com"),
        })?;
        db.connection.execute(
            "UPDATE drafts SET updated_at = CASE id WHEN ?1 THEN 300 WHEN ?2 THEN 200 END",
            (first_id, second_id),
        )?;

        let drafts = db.get_drafts()?;
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].id, first_id);
        assert_eq!(drafts[0].subject, "Updated subject");
        assert_eq!(drafts[0].to_field, "dave@example.com");
        assert_eq!(drafts[0].content, "Updated body");
        assert_eq!(drafts[0].parent_events, replacement_parents);
        assert_eq!(drafts[0].selected_account.as_deref(), Some("account-b"));
        assert_eq!(
            drafts[0].selected_nip05.as_deref(),
            Some("dave@example.com")
        );
        assert_eq!(drafts[1].id, second_id);
        assert_eq!(drafts[1].subject, "Second subject");
        assert_eq!(drafts[1].to_field, "carol@example.com");
        assert_eq!(drafts[1].content, "Second body");
        assert_eq!(drafts[1].parent_events, second_parents);
        assert_eq!(drafts[1].selected_account, None);
        assert_eq!(drafts[1].selected_nip05, None);

        db.delete_draft(second_id)?;
        db.delete_draft(second_id)?;
        let remaining = db.get_drafts()?;
        assert_eq!(db.get_draft_count()?, 1);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, first_id);

        Ok(())
    }
}
