use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use tokio::sync::RwLock;
use uuid::Uuid;
use anyhow::{Result, anyhow};

use crate::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthType {
    GeminiApiKey,
    VertexAi,
    ExistingLogin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRequest {
    pub auth_type: AuthType,
    pub config: serde_json::Value,
    pub workspace_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub auth_type: AuthType,
    pub config: serde_json::Value,
    pub workspace_path: String,
    pub authenticated: bool,
}

pub struct AuthManager {
    auth_info: RwLock<HashMap<Uuid, AuthInfo>>,
    config: Config,
}

impl AuthManager {
    pub fn new(config: Config) -> Self {
        Self {
            auth_info: RwLock::new(HashMap::new()),
            config,
        }
    }

    pub fn auth_file_path(&self) -> Option<&PathBuf> {
        self.config.auth_file.as_ref()
    }

    pub async fn authenticate(&self, workspace_id: Uuid, request: AuthRequest) -> Result<AuthResponse> {
        let auth_info = AuthInfo {
            auth_type: request.auth_type.clone(),
            config: request.config.clone(),
            workspace_path: request.workspace_path,
            authenticated: false,
        };

        // Validate authentication based on type
        let validated = match request.auth_type {
            AuthType::GeminiApiKey => self.validate_gemini_api_key(&request.config).await?,
            AuthType::VertexAi => self.validate_vertex_ai(&request.config).await?,
            AuthType::ExistingLogin => self.validate_existing_login(&request.config).await?,
        };

        if validated {
            let mut auth_info = auth_info;
            auth_info.authenticated = true;
            
            self.auth_info.write().await.insert(workspace_id, auth_info);
            
            Ok(AuthResponse {
                success: true,
                message: "Authentication successful".to_string(),
            })
        } else {
            Err(anyhow!("Authentication failed"))
        }
    }

    pub async fn is_authenticated(&self, workspace_id: Uuid) -> bool {
        self.auth_info
            .read()
            .await
            .get(&workspace_id)
            .map(|info| info.authenticated)
            .unwrap_or(false)
    }

    pub async fn get_auth_info(&self, workspace_id: Uuid) -> Option<AuthInfo> {
        self.auth_info.read().await.get(&workspace_id).cloned()
    }

    async fn validate_gemini_api_key(&self, config: &serde_json::Value) -> Result<bool> {
        let api_key = config
            .get("api_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing API key"))?;

        // Basic validation - check if key is not empty and has reasonable format
        if api_key.is_empty() || api_key.len() < 10 {
            return Ok(false);
        }

        // TODO: Make actual API call to validate key
        // For now, just check format
        Ok(api_key.starts_with("AI") || api_key.len() > 20)
    }

    async fn validate_vertex_ai(&self, config: &serde_json::Value) -> Result<bool> {
        let project_id = config
            .get("project_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing project_id"))?;

        let location = config
            .get("location")
            .and_then(|v| v.as_str())
            .unwrap_or("us-central1");

        // Basic validation
        if project_id.is_empty() {
            return Ok(false);
        }

        // TODO: Validate Vertex AI credentials
        // For now, just check if we have required fields
        Ok(!project_id.is_empty() && !location.is_empty())
    }

    async fn validate_existing_login(&self, config: &serde_json::Value) -> Result<bool> {
        let login_path = if let Some(login_path) = config.get("login_path").and_then(|v| v.as_str()) {
            // Use provided path
            PathBuf::from(login_path)
        } else if let Some(auth_file) = &self.config.auth_file {
            // Use configured auth file path
            auth_file.clone()
        } else {
            // Use default location - try to detect standard Gemini CLI auth file
            let home_dir = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))
                .map_err(|_| anyhow!("Could not determine home directory"))?;
            PathBuf::from(home_dir).join(".gemini").join("auth.json")
        };

        // Check if the login file exists
        Ok(tokio::fs::metadata(&login_path).await.is_ok())
    }
}