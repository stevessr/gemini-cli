#!/bin/bash

set -e

echo "Building Gemini CLI Web Service..."

# Build the main Gemini CLI packages first
cd ..
echo "Building Gemini CLI core packages..."
npm run build

# Install bridge dependencies
cd web-service/bridge
echo "Installing Node.js bridge dependencies..."
npm install

# Build Rust service
cd ..
echo "Building Rust web service..."
cargo build --release

echo "Build complete!"
echo ""
echo "To start the web service:"
echo "  cd web-service"
echo "  cargo run --release"
echo ""
echo "The web interface will be available at http://localhost:3000"