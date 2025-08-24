use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use uuid::Uuid;
use anyhow::{Result, anyhow};
// use futures_util::{SinkExt, StreamExt}; // Commented out WebSocket imports for now

use crate::{AppState, PendingApproval};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub content: String,
    pub is_user: bool,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct ChatSession {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub messages: Vec<ChatMessage>,
    pub pending_approvals: HashMap<Uuid, PendingApproval>,
    pub auto_approve: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatResponse {
    pub message_id: Uuid,
    pub ai_response: Option<String>,
    pub pending_approvals: Vec<PendingApproval>,
}

pub struct ChatManager {
    sessions: RwLock<HashMap<Uuid, ChatSession>>,
}

impl ChatManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn create_session(&self, workspace_id: Uuid) -> Result<Uuid> {
        let session_id = Uuid::new_v4();
        let session = ChatSession {
            id: session_id,
            workspace_id,
            messages: Vec::new(),
            pending_approvals: HashMap::new(),
            auto_approve: false,
        };

        self.sessions.write().await.insert(session_id, session);
        Ok(session_id)
    }

    pub async fn send_message(
        &self,
        session_id: Uuid,
        message: ChatMessage,
        auto_approve: bool,
    ) -> Result<ChatResponse> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| anyhow!("Session not found"))?;

        session.messages.push(message.clone());
        session.auto_approve = auto_approve;

        // TODO: Integrate with Node.js bridge to send message to Gemini CLI core
        // For now, simulate a response
        let ai_response = self.simulate_ai_response(&message.content, auto_approve).await?;
        
        let ai_message = ChatMessage {
            id: Uuid::new_v4(),
            content: ai_response.clone(),
            is_user: false,
            timestamp: chrono::Utc::now(),
        };
        
        session.messages.push(ai_message);

        // Simulate pending approvals if auto_approve is false
        let pending_approvals = if !auto_approve && message.content.contains("file") {
            vec![PendingApproval {
                id: Uuid::new_v4(),
                tool_name: "edit_file".to_string(),
                tool_args: serde_json::json!({
                    "path": "example.txt",
                    "content": "new content"
                }),
                description: "Edit file example.txt".to_string(),
            }]
        } else {
            vec![]
        };

        Ok(ChatResponse {
            message_id: message.id,
            ai_response: Some(ai_response),
            pending_approvals,
        })
    }

    pub async fn handle_approval(
        &self,
        session_id: Uuid,
        approval_id: Uuid,
        approved: bool,
        auto_approve_future: Option<bool>,
    ) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| anyhow!("Session not found"))?;

        if let Some(approval) = session.pending_approvals.remove(&approval_id) {
            if approved {
                // Execute the tool
                let result = self.execute_tool(&approval).await?;
                
                let result_message = ChatMessage {
                    id: Uuid::new_v4(),
                    content: format!("Tool execution result: {}", result),
                    is_user: false,
                    timestamp: chrono::Utc::now(),
                };
                
                session.messages.push(result_message);
            }

            if let Some(auto_approve) = auto_approve_future {
                session.auto_approve = auto_approve;
            }
        }

        Ok(())
    }

    pub async fn get_session_history(&self, session_id: Uuid) -> Result<Vec<ChatMessage>> {
        let sessions = self.sessions.read().await;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| anyhow!("Session not found"))?;

        Ok(session.messages.clone())
    }

    async fn simulate_ai_response(&self, input: &str, _auto_approve: bool) -> Result<String> {
        // TODO: Replace with actual Gemini API call through Node.js bridge
        let response = match input.to_lowercase() {
            s if s.contains("hello") => "Hello! I'm Gemini, how can I help you today?",
            s if s.contains("file") => "I can help you with file operations. What would you like to do?",
            s if s.contains("code") => "I'd be happy to help with your code. Please share what you're working on.",
            _ => "I understand. Let me help you with that.",
        };

        Ok(response.to_string())
    }

    async fn execute_tool(&self, approval: &PendingApproval) -> Result<String> {
        // TODO: Implement actual tool execution through Node.js bridge
        match approval.tool_name.as_str() {
            "edit_file" => Ok("File edited successfully".to_string()),
            "run_command" => Ok("Command executed successfully".to_string()),
            _ => Ok("Tool executed successfully".to_string()),
        }
    }
}

// WebSocket handler temporarily disabled
/*
pub async fn handle_websocket_connection(
    mut socket: WebSocket,
    state: AppState,
    session_id: Uuid,
) {
    // Implementation will be added when WebSocket issues are resolved
}
*/