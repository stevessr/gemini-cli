#!/usr/bin/env node

/**
 * @license
 * Copyright 2025 Google LLC
 * SPDX-License-Identifier: Apache-2.0
 */

import { Config, AuthType, sessionId } from '@google/gemini-cli-core';
import { loadSettings } from '../packages/cli/src/config/settings.js';
import { loadExtensions } from '../packages/cli/src/config/extension.js';
import { loadCliConfig } from '../packages/cli/src/config/loadCliConfig.js';
import process from 'process';
import fs from 'fs';

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
      
      // Load settings for the workspace
      const settings = this.loadSettingsForWorkspace(workspace_path);
      
      // Load extensions
      const extensions = this.loadExtensionsForWorkspace(workspace_path);
      
      // Create configuration with empty argv (non-interactive mode)
      const argv = {
        promptInteractive: false,
        useExternalAuth: false,
        debug: false,
        nonInteractive: true,
      };

      const config = await loadCliConfig(
        settings.merged,
        extensions,
        sessionId,
        argv
      );

      // Set up authentication if provided
      if (auth_info) {
        await this.setupAuthentication(config, auth_info);
      }

      const sessionKey = `${workspace_path}-${Date.now()}`;
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

      // TODO: Integrate with Gemini CLI core to actually send the message
      // For now, simulate a response based on the existing CLI functionality
      const response = await this.simulateGeminiResponse(message, session.config);

      // Add AI response to session
      session.messages.push({
        role: 'assistant',
        content: response,
        timestamp: new Date().toISOString(),
      });

      this.sendSuccess(requestId, { response });
    } catch (error) {
      this.sendError(requestId, `Failed to send message: ${error.message}`);
    }
  }

  async executeTool(requestId, params) {
    try {
      const { tool_name, args } = params;
      
      // TODO: Integrate with actual tool execution from Core package
      // For now, simulate tool execution
      const result = `Executed ${tool_name} with args: ${JSON.stringify(args)}`;
      
      this.sendSuccess(requestId, { result });
    } catch (error) {
      this.sendError(requestId, `Failed to execute tool: ${error.message}`);
    }
  }

  async getAuthInfo(requestId, params) {
    try {
      const { workspace_path } = params;
      
      // Check for existing authentication files
      const geminiDir = `${workspace_path}/.gemini`;
      const userHome = process.env.HOME || process.env.USERPROFILE;
      const globalGeminiDir = `${userHome}/.gemini`;
      
      const authInfo = {
        has_local_auth: fs.existsSync(`${geminiDir}/auth`),
        has_global_auth: fs.existsSync(`${globalGeminiDir}/auth`),
        available_auth_types: ['GeminiApiKey', 'VertexAi', 'ExistingLogin'],
      };
      
      this.sendSuccess(requestId, authInfo);
    } catch (error) {
      this.sendError(requestId, `Failed to get auth info: ${error.message}`);
    }
  }

  loadSettingsForWorkspace(workspacePath) {
    try {
      // Use the same settings loading logic as the CLI
      return loadSettings(workspacePath);
    } catch (error) {
      // Return minimal settings if loading fails
      return {
        merged: {},
        errors: [],
      };
    }
  }

  loadExtensionsForWorkspace(workspacePath) {
    try {
      return loadExtensions(workspacePath);
    } catch (error) {
      return [];
    }
  }

  async setupAuthentication(config, authInfo) {
    const { auth_type, config: authConfig } = authInfo;
    
    switch (auth_type) {
      case 'GeminiApiKey':
        process.env.GEMINI_API_KEY = authConfig.api_key;
        await config.refreshAuth(AuthType.USE_GEMINI);
        break;
      case 'VertexAi':
        process.env.GOOGLE_CLOUD_PROJECT = authConfig.project_id;
        process.env.GOOGLE_CLOUD_LOCATION = authConfig.location || 'us-central1';
        await config.refreshAuth(AuthType.USE_VERTEX_AI);
        break;
      case 'ExistingLogin':
        // The existing login should already be available in the standard location
        await config.refreshAuth(AuthType.LOGIN_WITH_GOOGLE);
        break;
    }
  }

  async simulateGeminiResponse(message, config) {
    // TODO: Replace with actual Gemini API call using the config
    // This would use config.getGeminiClient() to get the authenticated client
    // and then send the message through the normal Gemini CLI flow
    
    const responses = [
      "I understand. Let me help you with that.",
      "I can assist you with various tasks including file operations, code analysis, and more.",
      "What specific task would you like me to help you with?",
      "I'm ready to work with your codebase. What would you like to do?",
    ];
    
    return responses[Math.floor(Math.random() * responses.length)];
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