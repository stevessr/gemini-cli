use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};
use tower_http::{cors::CorsLayer, services::ServeDir};
use tracing::{info, Level};
use uuid::Uuid;

mod auth;
mod chat;
mod node_bridge;
mod workspace;

use auth::{AuthManager, AuthRequest, AuthType};
use chat::{ChatManager, ChatMessage};
use node_bridge::NodeBridge;
use workspace::{WorkspaceManager, WorkspaceInfo};

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
    chat_manager: Arc<ChatManager>,
    workspace_manager: Arc<WorkspaceManager>,
    node_bridge: Arc<NodeBridge>,
    config: Config,
}

#[derive(Serialize, Deserialize)]
struct CreateSessionRequest {
    workspace_path: String,
    auth_type: Option<AuthType>,
    auth_config: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
struct CreateSessionResponse {
    session_id: Uuid,
    workspace_id: Uuid,
}

#[derive(Serialize, Deserialize)]
struct SendMessageRequest {
    content: String,
    auto_approve: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct SendMessageResponse {
    message_id: Uuid,
    response: Option<String>,
    pending_approvals: Vec<PendingApproval>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingApproval {
    id: Uuid,
    tool_name: String,
    tool_args: serde_json::Value,
    description: String,
}

#[derive(Serialize, Deserialize)]
struct ApprovalRequest {
    approval_id: Uuid,
    approved: bool,
    auto_approve_future: Option<bool>,
}

async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<CreateSessionResponse>, StatusCode> {
    // Create or get workspace
    let workspace_id = state
        .workspace_manager
        .create_workspace(&req.workspace_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Authenticate if auth config provided
    if let (Some(auth_type), Some(auth_config)) = (req.auth_type, req.auth_config) {
        let auth_req = AuthRequest {
            auth_type,
            config: auth_config,
            workspace_path: req.workspace_path.clone(),
        };
        
        state
            .auth_manager
            .authenticate(workspace_id, auth_req)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;
    }

    // Create chat session
    let session_id = state
        .chat_manager
        .create_session(workspace_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(CreateSessionResponse {
        session_id,
        workspace_id,
    }))
}

async fn send_message(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, StatusCode> {
    let message = ChatMessage {
        id: Uuid::new_v4(),
        content: req.content,
        is_user: true,
        timestamp: chrono::Utc::now(),
    };

    let response = state
        .chat_manager
        .send_message(session_id, message, req.auto_approve.unwrap_or(false))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(SendMessageResponse {
        message_id: response.message_id,
        response: response.ai_response,
        pending_approvals: response.pending_approvals,
    }))
}

async fn approve_tool(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Json(req): Json<ApprovalRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .chat_manager
        .handle_approval(session_id, req.approval_id, req.approved, req.auto_approve_future)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

async fn list_workspaces(
    State(state): State<AppState>,
) -> Result<Json<Vec<WorkspaceInfo>>, StatusCode> {
    let workspaces = state.workspace_manager.list_workspaces().await;
    Ok(Json(workspaces))
}

async fn get_session_history(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> Result<Json<Vec<ChatMessage>>, StatusCode> {
    let history = state
        .chat_manager
        .get_session_history(session_id)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(history))
}

// WebSocket handler temporarily disabled
/*
async fn handle_websocket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| chat::handle_websocket_connection(socket, state, session_id))
}
*/

async fn serve_frontend() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    let config = Config {
        host: args.host,
        port: args.port,
        gemini_cli_path: args.gemini_cli_path,
        auth_file: args.auth_file,
    };

    info!("Starting Gemini Web Service with configuration:");
    info!("  Host: {}", config.host);
    info!("  Port: {}", config.port);
    if let Some(ref cli_path) = config.gemini_cli_path {
        info!("  Gemini CLI Path: {}", cli_path.display());
    }
    if let Some(ref auth_file) = config.auth_file {
        info!("  Auth File: {}", auth_file.display());
    }

    let app_state = AppState {
        auth_manager: Arc::new(AuthManager::new(config.clone())),
        chat_manager: Arc::new(ChatManager::new()),
        workspace_manager: Arc::new(WorkspaceManager::new()),
        node_bridge: Arc::new(NodeBridge::new(config.clone())),
        config: config.clone(),
    };

    let app = Router::new()
        .route("/", get(serve_frontend))
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/:session_id/messages", post(send_message))
        .route("/api/sessions/:session_id/approve", post(approve_tool))
        .route("/api/sessions/:session_id/history", get(get_session_history))
        // Remove the websocket route for now since it's causing issues
        // .route("/api/sessions/:session_id/ws", get(handle_websocket))
        .route("/api/workspaces", get(list_workspaces))
        .nest_service("/static", ServeDir::new("web-service/static"))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let addr = SocketAddr::from((config.host, config.port));
    info!("Gemini Web Service listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}