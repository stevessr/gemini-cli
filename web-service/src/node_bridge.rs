use serde::{Deserialize, Serialize};
use anyhow::{Result, anyhow};
use uuid::Uuid;
use std::path::PathBuf;

use crate::Config;

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeRequest {
    pub id: Uuid,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeResponse {
    pub id: Uuid,
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

pub struct NodeBridge {
    config: Config,
}

impl NodeBridge {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn gemini_cli_path(&self) -> Option<&PathBuf> {
        self.config.gemini_cli_path.as_ref()
    }

    pub async fn start_bridge(&self) -> Result<()> {
        // TODO: Start Node.js bridge process that can communicate with Gemini CLI core
        // This would spawn a Node.js process that imports and uses the Core package
        // Use self.config.gemini_cli_path if provided, otherwise use default location
        if let Some(cli_path) = &self.config.gemini_cli_path {
            tracing::info!("Using Gemini CLI from: {}", cli_path.display());
        } else {
            tracing::info!("Using default Gemini CLI location");
        }
        Ok(())
    }

    pub async fn send_request(&self, request: NodeRequest) -> Result<NodeResponse> {
        // TODO: Send request to Node.js bridge process
        // For now, simulate a response
        Ok(NodeResponse {
            id: request.id,
            success: true,
            data: Some(serde_json::json!({
                "message": "Bridge not yet implemented",
                "method": request.method
            })),
            error: None,
        })
    }

    pub async fn create_chat_session(&self, workspace_path: &str, auth_info: serde_json::Value) -> Result<String> {
        let request = NodeRequest {
            id: Uuid::new_v4(),
            method: "create_session".to_string(),
            params: serde_json::json!({
                "workspace_path": workspace_path,
                "auth_info": auth_info
            }),
        };

        let response = self.send_request(request).await?;
        
        if response.success {
            let session_id = response.data
                .as_ref()
                .and_then(|d| d.get("session_id"))
                .and_then(|s| s.as_str())
                .unwrap_or("default")
                .to_string();
            Ok(session_id)
        } else {
            Err(anyhow!("Failed to create session: {:?}", response.error))
        }
    }

    pub async fn send_message(&self, session_id: &str, message: &str) -> Result<String> {
        let request = NodeRequest {
            id: Uuid::new_v4(),
            method: "send_message".to_string(),
            params: serde_json::json!({
                "session_id": session_id,
                "message": message
            }),
        };

        let response = self.send_request(request).await?;
        
        if response.success {
            let response_text = response.data
                .as_ref()
                .and_then(|d| d.get("response"))
                .and_then(|r| r.as_str())
                .unwrap_or("No response")
                .to_string();
            Ok(response_text)
        } else {
            Err(anyhow!("Failed to send message: {:?}", response.error))
        }
    }

    pub async fn execute_tool(&self, tool_name: &str, args: serde_json::Value) -> Result<String> {
        let request = NodeRequest {
            id: Uuid::new_v4(),
            method: "execute_tool".to_string(),
            params: serde_json::json!({
                "tool_name": tool_name,
                "args": args
            }),
        };

        let response = self.send_request(request).await?;
        
        if response.success {
            let result_text = response.data
                .as_ref()
                .and_then(|d| d.get("result"))
                .and_then(|r| r.as_str())
                .unwrap_or("Tool executed")
                .to_string();
            Ok(result_text)
        } else {
            Err(anyhow!("Failed to execute tool: {:?}", response.error))
        }
    }
}