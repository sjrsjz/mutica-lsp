#!/bin/bash
# 配置脚本：自动设置 Mutica LSP 服务器路径

set -e

# 获取脚本所在目录（即 mutica-lsp 项目根目录）
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
SERVER_PATH="${SCRIPT_DIR}/target/release/mutica-lsp"

echo "==================================="
echo "Mutica LSP 配置脚本"
echo "==================================="
echo ""

# 1. 检查服务器是否已构建
echo "1. 检查 LSP 服务器..."
if [ -f "$SERVER_PATH" ]; then
    echo "✓ LSP 服务器已存在: $SERVER_PATH"
else
    echo "✗ LSP 服务器不存在，开始构建..."
    cargo build --release
    if [ -f "$SERVER_PATH" ]; then
        echo "✓ LSP 服务器构建成功"
    else
        echo "✗ 构建失败"
        exit 1
    fi
fi

# 2. 检查执行权限
echo ""
echo "2. 检查执行权限..."
if [ -x "$SERVER_PATH" ]; then
    echo "✓ LSP 服务器有执行权限"
else
    echo "! 添加执行权限..."
    chmod +x "$SERVER_PATH"
    echo "✓ 执行权限已添加"
fi

# 3. 配置 VS Code 设置
echo ""
echo "3. 配置 VS Code 设置..."

VSCODE_DIR="${SCRIPT_DIR}/.vscode"
SETTINGS_FILE="${VSCODE_DIR}/settings.json"

mkdir -p "$VSCODE_DIR"

# 如果 settings.json 不存在，创建它
if [ ! -f "$SETTINGS_FILE" ]; then
    cat > "$SETTINGS_FILE" << EOF
{
    "muticaLsp.serverPath": "\${workspaceFolder}/target/release/mutica-lsp",
    "muticaLsp.trace.server": "verbose"
}
EOF
    echo "✓ 创建了 .vscode/settings.json"
else
    echo "! .vscode/settings.json 已存在，跳过创建"
    echo "  当前内容:"
    cat "$SETTINGS_FILE" | sed 's/^/  /'
fi

# 4. 编译扩展
echo ""
echo "4. 编译 VS Code 扩展..."
if command -v pnpm &> /dev/null; then
    if [ -d "$SCRIPT_DIR/node_modules" ]; then
        pnpm run compile
        echo "✓ 扩展编译成功"
    else
        echo "! node_modules 不存在，先安装依赖..."
        pnpm install
        pnpm run compile
        echo "✓ 依赖安装并编译成功"
    fi
else
    echo "✗ pnpm 未安装，请先安装 pnpm"
    exit 1
fi

# 5. 显示测试说明
echo ""
echo "==================================="
echo "✓ 配置完成！"
echo "==================================="
echo ""
echo "下一步："
echo "1. 在 VS Code 中打开此项目目录: $SCRIPT_DIR"
echo "2. 按 F5 启动调试，或安装扩展"
echo "3. 创建一个 .mu 文件进行测试"
echo "4. 查看输出面板 (Ctrl+Shift+U) 选择 'Mutica Language Server'"
echo ""
echo "如果在其他工作区使用，请添加以下配置到该工作区的 settings.json:"
echo ""
echo "{"
echo "  \"muticaLsp.serverPath\": \"$SERVER_PATH\""
echo "}"
echo ""
