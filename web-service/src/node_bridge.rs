use serde::{Deserialize, Serialize};
use anyhow::{Result, anyhow};
use uuid::Uuid;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{Config, auth::AuthRequest};

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeRequest {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeResponse {
    pub id: String,
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgeResponse {
    pub response: String,
    pub pending_approvals: Vec<BridgePendingApproval>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BridgePendingApproval {
    pub tool_name: String,
    pub args: serde_json::Value,
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    pub result: String,
}

pub struct NodeBridge {
    config: Config,
    bridge_process: Arc<Mutex<Option<Child>>>,
}

impl NodeBridge {
    pub async fn new(config: Config) -> Result<Self> {
        let bridge = Self {
            config,
            bridge_process: Arc::new(Mutex::new(None)),
        };
        
        // Start the bridge process
        bridge.start_bridge().await?;
        
        Ok(bridge)
    }

    async fn start_bridge(&self) -> Result<()> {
        let bridge_path = std::path::Path::new("bridge/bridge.js");
        
        if !bridge_path.exists() {
            return Err(anyhow!("Bridge script not found at: {}", bridge_path.display()));
        }

        let mut cmd = Command::new("node");
        cmd.arg(bridge_path)
           .stdin(Stdio::piped())
           .stdout(Stdio::piped())
           .stderr(Stdio::piped());

        // Add environment variables for configuration
        if let Some(cli_path) = &self.config.gemini_cli_path {
            cmd.env("GEMINI_CLI_PATH", cli_path);
        }
        if let Some(auth_file) = &self.config.auth_file {
            cmd.env("GEMINI_AUTH_FILE", auth_file);
        }

        let child = cmd.spawn()
            .map_err(|e| anyhow!("Failed to start bridge process: {}", e))?;

        *self.bridge_process.lock().await = Some(child);
        
        tracing::info!("Node.js bridge process started");
        Ok(())
    }

    async fn send_request(&self, request: NodeRequest) -> Result<NodeResponse> {
        let mut bridge_lock = self.bridge_process.lock().await;
        let bridge = bridge_lock.as_mut().ok_or_else(|| anyhow!("Bridge process not started"))?;

        let stdin = bridge.stdin.as_mut().ok_or_else(|| anyhow!("No stdin for bridge"))?;
        
        // Send request
        let request_json = serde_json::to_string(&request)?;
        stdin.write_all(request_json.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        // Read response
        let stdout = bridge.stdout.as_mut().ok_or_else(|| anyhow!("No stdout for bridge"))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: NodeResponse = serde_json::from_str(&line.trim())
            .map_err(|e| anyhow!("Failed to parse bridge response: {}", e))?;

        Ok(response)
    }

    pub async fn create_session(&self, session_id: &str, workspace_path: &str, auth_info: Option<AuthRequest>) -> Result<()> {
        let request = NodeRequest {
            id: session_id.to_string(),
            method: "create_session".to_string(),
            params: serde_json::json!({
                "workspace_path": workspace_path,
                "auth_info": auth_info
            }),
        };

        let response = self.send_request(request).await?;
        
        if response.success {
            Ok(())
        } else {
            Err(anyhow!("Failed to create session: {:?}", response.error))
        }
    }

    pub async fn send_message(&self, session_id: &str, message: &str) -> Result<BridgeResponse> {
        let request = NodeRequest {
            id: Uuid::new_v4().to_string(),
            method: "send_message".to_string(),
            params: serde_json::json!({
                "session_id": session_id,
                "message": message
            }),
        };

        let response = self.send_request(request).await?;
        
        if response.success {
            let data = response.data.ok_or_else(|| anyhow!("No data in response"))?;
            
            let response_text = data.get("response")
                .and_then(|r| r.as_str())
                .unwrap_or("No response")
                .to_string();
            
            let pending_approvals = data.get("pending_approvals")
                .and_then(|a| a.as_array())
                .map(|arr| arr.iter().filter_map(|item| {
                    Some(BridgePendingApproval {
                        tool_name: item.get("tool_name")?.as_str()?.to_string(),
                        args: item.get("args")?.clone(),
                        description: item.get("description")?.as_str()?.to_string(),
                    })
                }).collect())
                .unwrap_or_default();

            Ok(BridgeResponse {
                response: response_text,
                pending_approvals,
            })
        } else {
            Err(anyhow!("Failed to send message: {:?}", response.error))
        }
    }

    pub async fn execute_tool(&self, session_id: &str, tool_name: &str, args: serde_json::Value, approved: bool) -> Result<ToolExecutionResult> {
        let request = NodeRequest {
            id: Uuid::new_v4().to_string(),
            method: "execute_tool".to_string(),
            params: serde_json::json!({
                "session_id": session_id,
                "tool_name": tool_name,
                "args": args,
                "approved": approved
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
            
            Ok(ToolExecutionResult {
                result: result_text,
            })
        } else {
            Err(anyhow!("Failed to execute tool: {:?}", response.error))
        }
    }

    pub async fn execute_command(&self, command: &str, workspace_path: &str) -> Result<String> {
        let request = NodeRequest {
            id: Uuid::new_v4().to_string(),
            method: "execute_command".to_string(),
            params: serde_json::json!({
                "command": command,
                "workspace_path": workspace_path
            }),
        };

        let response = self.send_request(request).await?;
        
        if response.success {
            let output = response.data
                .as_ref()
                .and_then(|d| d.get("output"))
                .and_then(|o| o.as_str())
                .unwrap_or("Command executed")
                .to_string();
            
            Ok(output)
        } else {
            Err(anyhow!("Failed to execute command: {:?}", response.error))
        }
    }
}