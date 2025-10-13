#!/bin/bash
set -e

echo "Building Mutica LSP Server..."

# 构建 Rust 项目
cargo build --release

# 创建 bin 目录
mkdir -p bin

# 复制二进制文件到 bin 目录
echo "Copying mutica-lsp binary to bin directory..."
cp target/release/mutica-lsp bin/

# 编译 TypeScript
echo "Compiling TypeScript extension..."
pnpm run compile

echo "Build complete! Binary is in ./bin/mutica-lsp"
echo "To package the extension, run: vsce package or npx @vscode/vsce package"
