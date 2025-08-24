# Gemini CLI Web Service

A Rust-based web service that provides all Gemini CLI functionality (except login) through a web interface.

## Features

- **Multi-workspace support**: Work with multiple projects simultaneously
- **Authentication options**: 
  - Custom Gemini API keys
  - Vertex AI integration
  - Reuse existing Gemini CLI login files
- **Command execution modes**:
  - Manual approval for each tool/command
  - Auto-approve mode for streamlined workflow
- **Real-time chat interface**: WebSocket-based communication for responsive interactions
- **Isolated build system**: Completely separate from main Gemini CLI dependencies

## Architecture

The web service consists of three main components:

1. **Rust Web Server** (`src/`): HTTP API server with WebSocket support
2. **Node.js Bridge** (`bridge/`): Interfaces with existing Gemini CLI Core package
3. **Web Frontend** (`static/`): Modern web interface replicating CLI functionality

## Building and Running

### Prerequisites

- Rust (latest stable)
- Node.js (>=20)
- Built Gemini CLI packages

### Build

```bash
./build.sh
```

This will:
1. Build the main Gemini CLI packages
2. Install Node.js bridge dependencies
3. Compile the Rust web service

### Run

```bash
cargo run --release
```

The web service will start on `http://localhost:3000`

## Usage

1. **Create a workspace**: Enter a local directory path and optionally configure authentication
2. **Start chatting**: Use the chat interface just like the CLI
3. **Manage tools**: Approve or reject tool executions, or enable auto-approve mode
4. **Multiple workspaces**: Switch between different projects in the sidebar

## Authentication

### Gemini API Key
- Enter your API key in the settings panel
- Select "Gemini API Key" from the auth dropdown

### Vertex AI
- Configure your Google Cloud project ID and location
- Select "Vertex AI" from the auth dropdown
- Ensure you have proper Vertex AI credentials configured

### Existing Login
- Reuse your existing Gemini CLI authentication
- Select "Existing Login" from the auth dropdown
- Point to your existing login file path

## API Endpoints

- `POST /api/sessions` - Create new chat session
- `POST /api/sessions/:id/messages` - Send message
- `POST /api/sessions/:id/approve` - Approve/reject tool execution
- `GET /api/sessions/:id/history` - Get chat history
- `GET /api/sessions/:id/ws` - WebSocket connection for real-time updates
- `GET /api/workspaces` - List all workspaces

## Development

### Project Structure

```
web-service/
├── src/                 # Rust web server
│   ├── main.rs         # Main server and routes
│   ├── auth.rs         # Authentication management
│   ├── chat.rs         # Chat session management
│   ├── workspace.rs    # Workspace management
│   └── node_bridge.rs  # Node.js bridge interface
├── bridge/             # Node.js bridge to Core
│   ├── package.json
│   └── bridge.js       # Bridge implementation
├── static/             # Web frontend
│   └── index.html      # Single-page application
├── Cargo.toml          # Rust dependencies
├── build.sh            # Build script
└── README.md           # This file
```

### Adding New Features

1. **New API endpoints**: Add routes in `src/main.rs`
2. **Authentication methods**: Extend `src/auth.rs`
3. **Tool integrations**: Modify `bridge/bridge.js` to interface with Core
4. **UI improvements**: Update `static/index.html`

## Isolation from Main CLI

This web service is designed to be completely isolated from the main Gemini CLI:

- **Separate build system**: Uses Cargo for Rust and separate npm for bridge
- **Independent dependencies**: No shared build artifacts
- **Modular integration**: Only interfaces with Core package through bridge
- **Standalone deployment**: Can be deployed without affecting CLI

## Security Considerations

- API keys and credentials are handled securely
- Authentication is validated before processing requests
- Tool executions require explicit approval (unless auto-approve is enabled)
- WebSocket connections are properly managed and cleaned up

## Troubleshooting

### Build Issues
- Ensure all Gemini CLI packages are built first
- Check that Node.js and Rust are properly installed
- Verify all dependencies are available

### Runtime Issues
- Check that the workspace paths exist and are accessible
- Verify authentication credentials are correct
- Ensure proper permissions for file operations

### Bridge Communication
- The Node.js bridge uses stdio for communication with Rust
- Check logs for any bridge initialization errors
- Verify Core package imports are working correctly