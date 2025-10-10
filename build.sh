#!/bin/bash

echo "Building Mutica LSP Server..."
cargo build --release

echo "Installing VS Code extension dependencies..."
pnpm install

echo "Compiling VS Code extension..."
pnpm run compile

echo "Build complete!"
echo "LSP Server binary: ./target/release/mutica-lsp"
echo "VS Code extension: ./out/extension.js"
