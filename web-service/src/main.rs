/**
 * @license
 * Copyright 2025 Google LLC
 * SPDX-License-Identifier: Apache-2.0
 */

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post, put, delete},
    Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::{error, info, Level};
use uuid::Uuid;

mod auth;
mod node_bridge;
mod database;

use auth::{AuthManager, AuthRequest};
use node_bridge::NodeBridge;
use database::Database;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 3000)]
    port: u16,

    /// IP address to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,

    /// Path to Gemini CLI installation directory
    #[arg(long)]
    gemini_cli_path: Option<PathBuf>,

    /// Path to Gemini CLI authentication file
    #[arg(long)]
    auth_file: Option<PathBuf>,

    /// Database file path
    #[arg(long, default_value = "gemini_web.db")]
    database: PathBuf,
}

#[derive(Clone)]
pub struct Config {
    pub host: IpAddr,
    pub port: u16,
    pub gemini_cli_path: Option<PathBuf>,
    pub auth_file: Option<PathBuf>,
}

#[derive(Clone)]
pub struct AppState {
    auth_manager: Arc<AuthManager>,
    node_bridge: Arc<NodeBridge>,
    config: Config,
    database: Database,
}

#[derive(Serialize, Deserialize)]
struct CreateSessionRequest {
    workspace_path: String,
    auth_info: Option<AuthRequest>,
}

#[derive(Serialize, Deserialize)]
struct CreateSessionResponse {
    session_id: Uuid,
}

#[derive(Serialize, Deserialize)]
struct SendMessageRequest {
    message: String,
}

#[derive(Serialize, Deserialize)]
struct SendMessageResponse {
    message_id: Uuid,
    response: String,
    pending_approvals: Vec<ApiPendingApproval>,
}

#[derive(Serialize, Deserialize)]
struct ApiPendingApproval {
    id: Uuid,
    tool_name: String,
    args: serde_json::Value,
    description: String,
}

#[derive(Serialize, Deserialize)]
struct ApprovalRequest {
    approved: bool,
}

#[derive(Serialize, Deserialize)]
struct UpdateWorkspaceRequest {
    name: Option<String>,
    path: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CommandExecutionRequest {
    command: String,
    workspace_path: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct CommandExecutionResponse {
    output: String,
    error: Option<String>,
}

async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<CreateSessionResponse>, StatusCode> {
    // Create workspace directory if it doesn't exist
    let workspace_path = std::path::Path::new(&req.workspace_path);
    if !workspace_path.exists() {
        if let Err(e) = std::fs::create_dir_all(&workspace_path) {
            error!("Failed to create workspace directory: {}", e);
            return Err(StatusCode::BAD_REQUEST);
        }
        info!("Created workspace directory: {}", req.workspace_path);
    }

    // Validate authentication if provided
    if let Some(auth_info) = &req.auth_info {
        match state.auth_manager.validate_auth(auth_info).await {
            Ok(false) => {
                error!("Authentication validation failed");
                return Err(StatusCode::UNAUTHORIZED);
            }
            Err(e) => {
                error!("Authentication error: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
            Ok(true) => {
                info!("Authentication validated successfully");
            }
        }
    }

    // Create database session
    match state.database.create_session(&req.workspace_path).await {
        Ok(session) => {
            info!("Created session {} for workspace {}", session.id, req.workspace_path);
            
            // Create bridge session
            if let Err(e) = state.node_bridge.create_session(&session.id.to_string(), &req.workspace_path, req.auth_info).await {
                error!("Failed to create bridge session: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }

            Ok(Json(CreateSessionResponse {
                session_id: session.id,
            }))
        }
        Err(e) => {
            error!("Failed to create session: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn send_message(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, StatusCode> {
    // Verify session exists
    if state.database.get_session(session_id).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Store user message
    match state.database.create_message(session_id, "user", &req.message).await {
        Ok(user_message) => {
            info!("Stored user message: {}", user_message.id);
        }
        Err(e) => {
            error!("Failed to store user message: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Send message through bridge
    match state.node_bridge.send_message(&session_id.to_string(), &req.message).await {
        Ok(response) => {
            // Store AI response
            if let Err(e) = state.database.create_message(session_id, "assistant", &response.response).await {
                error!("Failed to store AI response: {}", e);
            }

            // Store pending approvals
            let mut api_approvals = Vec::new();
            for approval in response.pending_approvals {
                match state.database.create_pending_approval(
                    session_id, 
                    &approval.tool_name, 
                    approval.args.clone(), 
                    &approval.description
                ).await {
                    Ok(stored_approval) => {
                        api_approvals.push(ApiPendingApproval {
                            id: stored_approval.id,
                            tool_name: approval.tool_name,
                            args: approval.args,
                            description: approval.description,
                        });
                    }
                    Err(e) => {
                        error!("Failed to store pending approval: {}", e);
                    }
                }
            }

            Ok(Json(SendMessageResponse {
                message_id: Uuid::new_v4(),
                response: response.response,
                pending_approvals: api_approvals,
            }))
        }
        Err(e) => {
            error!("Failed to send message through bridge: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn approve_tool(
    State(state): State<AppState>,
    Path((session_id, approval_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<ApprovalRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Get the pending approval
    let approval = match state.database.get_pending_approval(approval_id).await {
        Ok(Some(approval)) => approval,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get pending approval: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Verify it belongs to the session
    if approval.session_id != session_id {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Update approval status
    if let Err(e) = state.database.approve_pending(approval_id, req.approved).await {
        error!("Failed to update approval status: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    if req.approved {
        // Execute the tool through bridge
        match state.node_bridge.execute_tool(&session_id.to_string(), &approval.tool_name, approval.args, true).await {
            Ok(result) => {
                // Store the tool execution result
                let result_message = format!("Tool '{}' executed successfully: {}", approval.tool_name, result.result);
                if let Err(e) = state.database.create_message(session_id, "assistant", &result_message).await {
                    error!("Failed to store tool result: {}", e);
                }

                Ok(Json(serde_json::json!({
                    "success": true,
                    "result": result.result
                })))
            }
            Err(e) => {
                error!("Failed to execute tool: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
    } else {
        Ok(Json(serde_json::json!({
            "success": true,
            "result": "Tool execution rejected"
        })))
    }
}

async fn get_session_history(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Verify session exists
    if state.database.get_session(session_id).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Get messages
    let messages = match state.database.get_session_messages(session_id).await {
        Ok(messages) => messages,
        Err(e) => {
            error!("Failed to get session messages: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Get pending approvals
    let pending_approvals = match state.database.get_pending_approvals(session_id).await {
        Ok(approvals) => approvals.into_iter().map(|a| ApiPendingApproval {
            id: a.id,
            tool_name: a.tool_name,
            args: a.args,
            description: a.description,
        }).collect::<Vec<_>>(),
        Err(e) => {
            error!("Failed to get pending approvals: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    Ok(Json(serde_json::json!({
        "messages": messages,
        "pending_approvals": pending_approvals
    })))
}

#[derive(Serialize, Deserialize)]
struct WorkspaceInfo {
    id: Uuid,
    path: String,
    name: String,
}

async fn update_workspace(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    Json(req): Json<UpdateWorkspaceRequest>,
) -> Result<Json<WorkspaceInfo>, StatusCode> {
    // Get the current session
    let session = match state.database.get_session(workspace_id).await {
        Ok(Some(session)) => session,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get session: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let mut new_path = session.workspace_path.clone();
    
    // Update path if provided
    if let Some(path) = req.path {
        // Create new directory if it doesn't exist
        if !std::path::Path::new(&path).exists() {
            if let Err(e) = std::fs::create_dir_all(&path) {
                error!("Failed to create new workspace directory: {}", e);
                return Err(StatusCode::BAD_REQUEST);
            }
        }
        new_path = path;
    }

    // Update the session in database
    if let Err(e) = state.database.update_session_path(workspace_id, &new_path).await {
        error!("Failed to update session path: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let path_name = std::path::Path::new(&new_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string();

    Ok(Json(WorkspaceInfo {
        id: workspace_id,
        path: new_path,
        name: req.name.unwrap_or(path_name),
    }))
}

async fn delete_workspace(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if let Err(e) = state.database.delete_session(workspace_id).await {
        error!("Failed to delete session: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

async fn execute_command(
    State(state): State<AppState>,
    Json(req): Json<CommandExecutionRequest>,
) -> Result<Json<CommandExecutionResponse>, StatusCode> {
    let workspace_path = req.workspace_path.unwrap_or_else(|| ".".to_string());
    
    // Execute command using gemini-cli through node bridge
    match state.node_bridge.execute_command(&req.command, &workspace_path).await {
        Ok(result) => {
            Ok(Json(CommandExecutionResponse {
                output: result,
                error: None,
            }))
        }
        Err(e) => {
            error!("Failed to execute command: {}", e);
            Ok(Json(CommandExecutionResponse {
                output: String::new(),
                error: Some(e.to_string()),
            }))
        }
    }
}

async fn list_workspaces(
    State(state): State<AppState>,
) -> Result<Json<Vec<WorkspaceInfo>>, StatusCode> {
    match state.database.list_sessions().await {
        Ok(sessions) => {
            let workspaces = sessions.into_iter().map(|session| {
                let path_name = std::path::Path::new(&session.workspace_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown")
                    .to_string();

                WorkspaceInfo {
                    id: session.id,
                    path: session.workspace_path,
                    name: path_name,
                }
            }).collect();

            Ok(Json(workspaces))
        }
        Err(e) => {
            error!("Failed to list workspaces: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn serve_frontend() -> Result<axum::response::Html<String>, StatusCode> {
    let html = std::fs::read_to_string("static/index.html")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(axum::response::Html(html))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    let config = Config {
        host: args.host,
        port: args.port,
        gemini_cli_path: args.gemini_cli_path.clone(),
        auth_file: args.auth_file.clone(),
    };

    info!("Starting Gemini Web Service");
    info!("Configuration:");
    info!("  Host: {}", config.host);
    info!("  Port: {}", config.port);
    info!("  Gemini CLI Path: {:?}", config.gemini_cli_path);
    info!("  Auth File: {:?}", config.auth_file);
    info!("  Database: {:?}", args.database);

    // Initialize database
    let database_url = format!("sqlite://{}", args.database.display());
    
    // Ensure database file exists - create it if necessary
    if !args.database.exists() {
        if let Some(parent) = args.database.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::File::create(&args.database)?;
    }
    
    let database = Database::new(&database_url).await?;
    info!("Database initialized");

    // Initialize components
    let auth_manager = Arc::new(AuthManager::new(config.clone()));
    let node_bridge = Arc::new(NodeBridge::new(config.clone()).await?);

    let app_state = AppState {
        auth_manager,
        node_bridge,
        config: config.clone(),
        database,
    };

    // Build application router
    let app = Router::new()
        .route("/", get(serve_frontend))
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/:id/messages", post(send_message))
        .route("/api/sessions/:session_id/approve/:approval_id", post(approve_tool))
        .route("/api/sessions/:id/history", get(get_session_history))
        .route("/api/sessions/:id", put(update_workspace))
        .route("/api/sessions/:id", delete(delete_workspace))
        .route("/api/workspaces", get(list_workspaces))
        .route("/api/execute", post(execute_command))
        .nest_service("/static", ServeDir::new("static"))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let addr = SocketAddr::from((config.host, config.port));
    info!("Server starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}