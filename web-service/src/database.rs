/**
 * @license
 * Copyright 2025 Google LLC
 * SPDX-License-Identifier: Apache-2.0
 */

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePool, Row};
use uuid::Uuid;

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub workspace_path: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub session_id: Uuid,
    pub role: String, // "user" or "assistant"
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: Uuid,
    pub session_id: Uuid,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub description: String,
    pub approved: Option<bool>,
    pub created_at: DateTime<Utc>,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = SqlitePool::connect(database_url).await?;
        
        // Run migrations
        sqlx::migrate!("./migrations").run(&pool).await?;
        
        Ok(Database { pool })
    }

    pub async fn create_session(&self, workspace_path: &str) -> Result<Session> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        sqlx::query(
            "INSERT INTO sessions (id, workspace_path, created_at, updated_at) VALUES ($1, $2, $3, $4)"
        )
        .bind(id)
        .bind(workspace_path)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(Session {
            id,
            workspace_path: workspace_path.to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get_session(&self, session_id: Uuid) -> Result<Option<Session>> {
        let row = sqlx::query("SELECT id, workspace_path, created_at, updated_at FROM sessions WHERE id = $1")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            Ok(Some(Session {
                id: row.get("id"),
                workspace_path: row.get("workspace_path"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn create_message(&self, session_id: Uuid, role: &str, content: &str) -> Result<Message> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        sqlx::query(
            "INSERT INTO messages (id, session_id, role, content, created_at) VALUES ($1, $2, $3, $4, $5)"
        )
        .bind(id)
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Update session timestamp
        sqlx::query("UPDATE sessions SET updated_at = $1 WHERE id = $2")
            .bind(now)
            .bind(session_id)
            .execute(&self.pool)
            .await?;

        Ok(Message {
            id,
            session_id,
            role: role.to_string(),
            content: content.to_string(),
            created_at: now,
        })
    }

    pub async fn get_session_messages(&self, session_id: Uuid) -> Result<Vec<Message>> {
        let rows = sqlx::query("SELECT id, session_id, role, content, created_at FROM messages WHERE session_id = $1 ORDER BY created_at")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let messages = rows.into_iter().map(|row| Message {
            id: row.get("id"),
            session_id: row.get("session_id"),
            role: row.get("role"),
            content: row.get("content"),
            created_at: row.get("created_at"),
        }).collect();

        Ok(messages)
    }

    pub async fn create_pending_approval(&self, session_id: Uuid, tool_name: &str, args: serde_json::Value, description: &str) -> Result<PendingApproval> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        
        sqlx::query(
            "INSERT INTO pending_approvals (id, session_id, tool_name, args, description, created_at) VALUES ($1, $2, $3, $4, $5, $6)"
        )
        .bind(id)
        .bind(session_id)
        .bind(tool_name)
        .bind(args.to_string())
        .bind(description)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(PendingApproval {
            id,
            session_id,
            tool_name: tool_name.to_string(),
            args,
            description: description.to_string(),
            approved: None,
            created_at: now,
        })
    }

    pub async fn get_pending_approvals(&self, session_id: Uuid) -> Result<Vec<PendingApproval>> {
        let rows = sqlx::query("SELECT id, session_id, tool_name, args, description, approved, created_at FROM pending_approvals WHERE session_id = $1 AND approved IS NULL ORDER BY created_at")
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;

        let approvals = rows.into_iter().map(|row| {
            let args_str: String = row.get("args");
            let args = serde_json::from_str(&args_str).unwrap_or(serde_json::Value::Null);
            
            PendingApproval {
                id: row.get("id"),
                session_id: row.get("session_id"),
                tool_name: row.get("tool_name"),
                args,
                description: row.get("description"),
                approved: row.get("approved"),
                created_at: row.get("created_at"),
            }
        }).collect();

        Ok(approvals)
    }

    pub async fn approve_pending(&self, approval_id: Uuid, approved: bool) -> Result<()> {
        sqlx::query("UPDATE pending_approvals SET approved = $1 WHERE id = $2")
            .bind(approved)
            .bind(approval_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn get_pending_approval(&self, approval_id: Uuid) -> Result<Option<PendingApproval>> {
        let row = sqlx::query("SELECT id, session_id, tool_name, args, description, approved, created_at FROM pending_approvals WHERE id = $1")
            .bind(approval_id)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(row) = row {
            let args_str: String = row.get("args");
            let args = serde_json::from_str(&args_str).unwrap_or(serde_json::Value::Null);
            
            Ok(Some(PendingApproval {
                id: row.get("id"),
                session_id: row.get("session_id"),
                tool_name: row.get("tool_name"),
                args,
                description: row.get("description"),
                approved: row.get("approved"),
                created_at: row.get("created_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let rows = sqlx::query("SELECT id, workspace_path, created_at, updated_at FROM sessions ORDER BY updated_at DESC")
            .fetch_all(&self.pool)
            .await?;

        let sessions = rows.into_iter().map(|row| Session {
            id: row.get("id"),
            workspace_path: row.get("workspace_path"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }).collect();

        Ok(sessions)
    }
}