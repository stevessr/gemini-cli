use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
}

pub struct AuthManager {
    config: Config,
}

impl AuthManager {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn validate_auth(&self, request: &AuthRequest) -> Result<bool> {
        match request.auth_type {
            AuthType::GeminiApiKey => self.validate_gemini_api_key(&request.config).await,
            AuthType::VertexAi => self.validate_vertex_ai(&request.config).await,
            AuthType::ExistingLogin => self.validate_existing_login(&request.config).await,
        }
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

        // Check format - Gemini API keys typically start with "AI" or are long strings
        Ok(api_key.starts_with("AI") || api_key.len() > 30)
    }

    async fn validate_vertex_ai(&self, config: &serde_json::Value) -> Result<bool> {
        let project_id = config
            .get("project_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing project_id"))?;

        let _location = config
            .get("location")
            .and_then(|v| v.as_str())
            .unwrap_or("us-central1");

        // Basic validation
        if project_id.is_empty() {
            return Ok(false);
        }

        // Check if project_id has valid format (Google Cloud project IDs are typically lowercase with hyphens)
        let valid_format = project_id.chars().all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '-');
        Ok(valid_format && project_id.len() >= 6 && project_id.len() <= 30)
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

        // Check if the login file exists and is readable
        match tokio::fs::metadata(&login_path).await {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(_) => Ok(false),
        }
    }
}