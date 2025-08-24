use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::RwLock;
use uuid::Uuid;
use anyhow::{Result, anyhow};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: Uuid,
    pub path: String,
    pub name: String,
    pub active_sessions: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub info: WorkspaceInfo,
    pub session_ids: Vec<Uuid>,
}

pub struct WorkspaceManager {
    workspaces: RwLock<HashMap<Uuid, Workspace>>,
    path_to_id: RwLock<HashMap<String, Uuid>>,
}

impl WorkspaceManager {
    pub fn new() -> Self {
        Self {
            workspaces: RwLock::new(HashMap::new()),
            path_to_id: RwLock::new(HashMap::new()),
        }
    }

    pub async fn create_workspace(&self, path: &str) -> Result<Uuid> {
        // Normalize path
        let path_buf = PathBuf::from(path);
        let canonical_path = path_buf
            .canonicalize()
            .unwrap_or_else(|_| path_buf.clone())
            .to_string_lossy()
            .to_string();

        // Check if workspace already exists
        if let Some(&existing_id) = self.path_to_id.read().await.get(&canonical_path) {
            return Ok(existing_id);
        }

        // Validate that path exists and is a directory
        let metadata = tokio::fs::metadata(&canonical_path).await
            .map_err(|_| anyhow!("Path does not exist: {}", canonical_path))?;

        if !metadata.is_dir() {
            return Err(anyhow!("Path is not a directory: {}", canonical_path));
        }

        let workspace_id = Uuid::new_v4();
        let name = PathBuf::from(&canonical_path)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("root"))
            .to_string_lossy()
            .to_string();

        let workspace_info = WorkspaceInfo {
            id: workspace_id,
            path: canonical_path.clone(),
            name,
            active_sessions: 0,
            created_at: chrono::Utc::now(),
        };

        let workspace = Workspace {
            info: workspace_info,
            session_ids: Vec::new(),
        };

        self.workspaces.write().await.insert(workspace_id, workspace);
        self.path_to_id.write().await.insert(canonical_path, workspace_id);

        Ok(workspace_id)
    }

    pub async fn get_workspace(&self, workspace_id: Uuid) -> Option<WorkspaceInfo> {
        self.workspaces
            .read()
            .await
            .get(&workspace_id)
            .map(|w| w.info.clone())
    }

    pub async fn list_workspaces(&self) -> Vec<WorkspaceInfo> {
        self.workspaces
            .read()
            .await
            .values()
            .map(|w| w.info.clone())
            .collect()
    }

    pub async fn add_session(&self, workspace_id: Uuid, session_id: Uuid) -> Result<()> {
        let mut workspaces = self.workspaces.write().await;
        let workspace = workspaces
            .get_mut(&workspace_id)
            .ok_or_else(|| anyhow!("Workspace not found"))?;

        workspace.session_ids.push(session_id);
        workspace.info.active_sessions = workspace.session_ids.len();

        Ok(())
    }

    pub async fn remove_session(&self, workspace_id: Uuid, session_id: Uuid) -> Result<()> {
        let mut workspaces = self.workspaces.write().await;
        let workspace = workspaces
            .get_mut(&workspace_id)
            .ok_or_else(|| anyhow!("Workspace not found"))?;

        workspace.session_ids.retain(|&id| id != session_id);
        workspace.info.active_sessions = workspace.session_ids.len();

        Ok(())
    }

    pub async fn get_workspace_by_path(&self, path: &str) -> Option<Uuid> {
        let path_buf = PathBuf::from(path);
        let canonical_path = path_buf
            .canonicalize()
            .unwrap_or_else(|_| path_buf.clone())
            .to_string_lossy()
            .to_string();

        self.path_to_id.read().await.get(&canonical_path).copied()
    }
}