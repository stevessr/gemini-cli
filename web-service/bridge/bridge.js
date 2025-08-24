#!/usr/bin/env node

/**
 * @license
 * Copyright 2025 Google LLC
 * SPDX-License-Identifier: Apache-2.0
 */

import { 
  Config, 
  AuthType, 
  sessionId, 
  DEFAULT_GEMINI_FLASH_MODEL,
  createConfig
} from '@google/gemini-cli-core';
import process from 'process';
import fs from 'fs';
import path from 'path';

class GeminiBridge {
  constructor() {
    this.sessions = new Map();
    this.configs = new Map();
  }

  async initialize() {
    // Set up stdin/stdout communication with Rust service
    process.stdin.setEncoding('utf8');
    process.stdout.write(JSON.stringify({ type: 'ready' }) + '\n');
    
    process.stdin.on('data', (data) => {
      const lines = data.toString().split('\n').filter(line => line.trim());
      for (const line of lines) {
        try {
          const request = JSON.parse(line);
          this.handleRequest(request);
        } catch (error) {
          this.sendError(null, `Invalid JSON: ${error.message}`);
        }
      }
    });
  }

  async handleRequest(request) {
    try {
      const { id, method, params } = request;

      switch (method) {
        case 'create_session':
          await this.createSession(id, params);
          break;
        case 'send_message':
          await this.sendMessage(id, params);
          break;
        case 'execute_tool':
          await this.executeTool(id, params);
          break;
        case 'get_auth_info':
          await this.getAuthInfo(id, params);
          break;
        case 'validate_auth':
          await this.validateAuth(id, params);
          break;
        default:
          this.sendError(id, `Unknown method: ${method}`);
      }
    } catch (error) {
      this.sendError(request.id, error.message);
    }
  }

  async createSession(requestId, params) {
    try {
      const { workspace_path, auth_info } = params;
      
      // Verify workspace path exists
      if (!fs.existsSync(workspace_path)) {
        this.sendError(requestId, `Workspace path does not exist: ${workspace_path}`);
        return;
      }

      // Create configuration using the core package
      const config = await createConfig({
        cwd: workspace_path,
        model: DEFAULT_GEMINI_FLASH_MODEL,
        nonInteractive: true
      });

      // Set up authentication if provided
      if (auth_info) {
        const authResult = await this.setupAuthentication(config, auth_info);
        if (!authResult.success) {
          this.sendError(requestId, authResult.error);
          return;
        }
      }

      const sessionKey = `${workspace_path}-${Date.now()}`;
      
      // Initialize Gemini client to validate authentication
      try {
        const client = config.getGeminiClient();
        await client.validateAuth(); // This will throw if auth is invalid
      } catch (error) {
        this.sendError(requestId, `Authentication validation failed: ${error.message}`);
        return;
      }

      this.sessions.set(sessionKey, {
        config,
        workspace_path,
        messages: [],
      });
      this.configs.set(sessionKey, config);

      this.sendSuccess(requestId, { session_id: sessionKey });
    } catch (error) {
      this.sendError(requestId, `Failed to create session: ${error.message}`);
    }
  }

  async sendMessage(requestId, params) {
    try {
      const { session_id, message } = params;
      const session = this.sessions.get(session_id);
      
      if (!session) {
        this.sendError(requestId, 'Session not found');
        return;
      }

      // Add user message to session
      session.messages.push({
        role: 'user',
        content: message,
        timestamp: new Date().toISOString(),
      });

      // Use actual Gemini CLI core to send the message
      const response = await this.sendMessageToGemini(message, session.config);

      // Add AI response to session
      session.messages.push({
        role: 'assistant',
        content: response.text,
        timestamp: new Date().toISOString(),
        pending_approvals: response.pending_approvals || []
      });

      this.sendSuccess(requestId, { 
        response: response.text, 
        pending_approvals: response.pending_approvals || []
      });
    } catch (error) {
      this.sendError(requestId, `Failed to send message: ${error.message}`);
    }
  }

  async executeTool(requestId, params) {
    try {
      const { session_id, tool_name, args, approved } = params;
      const session = this.sessions.get(session_id);
      
      if (!session) {
        this.sendError(requestId, 'Session not found');
        return;
      }
      
      if (!approved) {
        this.sendError(requestId, 'Tool execution not approved');
        return;
      }

      // Execute the tool using the config's tool registry
      const toolRegistry = session.config.getToolRegistry();
      const tool = toolRegistry.getTool(tool_name);
      
      if (!tool) {
        this.sendError(requestId, `Tool not found: ${tool_name}`);
        return;
      }

      const result = await tool.execute(args, session.config);
      
      this.sendSuccess(requestId, { result });
    } catch (error) {
      this.sendError(requestId, `Failed to execute tool: ${error.message}`);
    }
  }

  async getAuthInfo(requestId, params) {
    try {
      const { workspace_path } = params;
      
      // Check for existing authentication files
      const geminiDir = path.join(workspace_path, '.gemini');
      const userHome = process.env.HOME || process.env.USERPROFILE;
      const globalGeminiDir = path.join(userHome, '.gemini');
      
      const authInfo = {
        has_local_auth: fs.existsSync(path.join(geminiDir, 'auth')),
        has_global_auth: fs.existsSync(path.join(globalGeminiDir, 'auth')),
        available_auth_types: ['GeminiApiKey', 'VertexAi', 'ExistingLogin'],
      };
      
      this.sendSuccess(requestId, authInfo);
    } catch (error) {
      this.sendError(requestId, `Failed to get auth info: ${error.message}`);
    }
  }

  async validateAuth(requestId, params) {
    try {
      const { auth_type, config: authConfig, workspace_path } = params;
      
      // Create a temporary config to test authentication
      const tempConfig = await createConfig({
        cwd: workspace_path || process.cwd(),
        model: DEFAULT_GEMINI_FLASH_MODEL,
        nonInteractive: true
      });

      const authResult = await this.setupAuthentication(tempConfig, { auth_type, config: authConfig });
      
      if (!authResult.success) {
        this.sendError(requestId, authResult.error);
        return;
      }

      // Try to validate the authentication
      try {
        const client = tempConfig.getGeminiClient();
        await client.validateAuth();
        this.sendSuccess(requestId, { valid: true });
      } catch (error) {
        this.sendError(requestId, `Authentication invalid: ${error.message}`);
      }
    } catch (error) {
      this.sendError(requestId, `Failed to validate auth: ${error.message}`);
    }
  }

  async setupAuthentication(config, authInfo) {
    const { auth_type, config: authConfig } = authInfo;
    
    try {
      switch (auth_type) {
        case 'GeminiApiKey':
          if (!authConfig.api_key) {
            return { success: false, error: 'API key is required' };
          }
          process.env.GEMINI_API_KEY = authConfig.api_key;
          await config.refreshAuth(AuthType.USE_GEMINI);
          break;
        case 'VertexAi':
          if (!authConfig.project_id) {
            return { success: false, error: 'Project ID is required for Vertex AI' };
          }
          process.env.GOOGLE_CLOUD_PROJECT = authConfig.project_id;
          process.env.GOOGLE_CLOUD_LOCATION = authConfig.location || 'us-central1';
          await config.refreshAuth(AuthType.USE_VERTEX_AI);
          break;
        case 'ExistingLogin':
          // The existing login should already be available in the standard location
          await config.refreshAuth(AuthType.LOGIN_WITH_GOOGLE);
          break;
        default:
          return { success: false, error: `Unknown auth type: ${auth_type}` };
      }
      return { success: true };
    } catch (error) {
      return { success: false, error: `Authentication setup failed: ${error.message}` };
    }
  }

  async sendMessageToGemini(message, config) {
    try {
      // Get the Gemini client from the config
      const client = config.getGeminiClient();
      
      // Create a content generator for streaming responses
      const contentGenerator = client.getContentGenerator();
      
      // Send the message and get response
      const response = await contentGenerator.generateContent([{
        role: 'user',
        parts: [{ text: message }]
      }]);

      // Extract text from response
      let responseText = '';
      if (response.candidates && response.candidates[0]) {
        const candidate = response.candidates[0];
        if (candidate.content && candidate.content.parts) {
          responseText = candidate.content.parts
            .filter(part => part.text)
            .map(part => part.text)
            .join('');
        }
      }

      // Check for tool calls that need approval
      const pendingApprovals = [];
      if (response.candidates && response.candidates[0]) {
        const candidate = response.candidates[0];
        if (candidate.content && candidate.content.parts) {
          for (const part of candidate.content.parts) {
            if (part.functionCall) {
              pendingApprovals.push({
                id: `tool-${Date.now()}-${Math.random()}`,
                tool_name: part.functionCall.name,
                args: part.functionCall.args,
                description: `Execute ${part.functionCall.name} with provided arguments`
              });
            }
          }
        }
      }

      return {
        text: responseText || 'I understand. Let me help you with that.',
        pending_approvals: pendingApprovals
      };
    } catch (error) {
      throw new Error(`Failed to send message to Gemini: ${error.message}`);
    }
  }

  sendSuccess(requestId, data) {
    const response = {
      id: requestId,
      success: true,
      data,
    };
    process.stdout.write(JSON.stringify(response) + '\n');
  }

  sendError(requestId, error) {
    const response = {
      id: requestId,
      success: false,
      error,
    };
    process.stdout.write(JSON.stringify(response) + '\n');
  }
}

// Start the bridge
const bridge = new GeminiBridge();
bridge.initialize().catch(error => {
  console.error('Failed to initialize bridge:', error);
  process.exit(1);
});