# Mutica LSP Server & VS Code Extension

这是一个使用 tower-lsp 构建的 Mutica 语言服务器协议 (LSP) 实现，以及对应的 VS Code 扩展。

## 项目结构

```
mutica-lsp/
├── src/                    # Rust LSP 服务器源码
│   └── main.rs
├── src-vscode/            # VS Code 扩展源码
│   └── extension.ts
├── syntaxes/              # 语法高亮定义
│   └── mutica.tmGrammar.json
├── out/                   # 编译后的 TypeScript 代码
├── target/                # 编译后的 Rust 二进制文件
├── Cargo.toml             # Rust 项目配置
├── package.json           # VS Code 扩展配置
├── tsconfig.json          # TypeScript 配置
└── build.sh               # 构建脚本
```

## 功能特性

- 基本的 LSP 功能（初始化、文档同步）
- 代码补全（示例实现）
- 语法高亮
- 文档事件处理（打开、更改、保存、关闭）
- 命令执行

## 快速开始

### 1. 构建项目

```bash
./build.sh
```

这将：
- 编译 Rust LSP 服务器
- 使用 pnpm 安装 Node.js 依赖
- 编译 TypeScript 扩展代码

### 2. 安装 VS Code 扩展

在 VS Code 中：
1. 按 `Ctrl+Shift+P` 打开命令面板
2. 运行 `Developer: Install Extension from Location...`
3. 选择这个项目的根目录

### 3. 配置 LSP 服务器路径

扩展会自动尝试在以下位置查找 LSP 服务器：
1. 用户配置的 `muticaLsp.serverPath`
2. 扩展安装目录下的 `target/release/mutica-lsp`
3. 工作区根目录下的 `target/release/mutica-lsp`
4. 各位置的 `target/debug/mutica-lsp` 版本

如果自动查找失败，可以在 VS Code 设置中手动配置路径：

**用户设置** (`settings.json`):
```json
{
  "muticaLsp.serverPath": "/absolute/path/to/mutica-lsp/target/release/mutica-lsp"
}
```

**工作区设置** (`.vscode/settings.json`):
```json
{
  "muticaLsp.serverPath": "${workspaceFolder}/target/release/mutica-lsp"
}
```

### 4. 测试

创建一个 `.mu` 文件并开始编辑，LSP 服务器应该会自动启动并提供语言支持。

## 故障排除

### LSP 服务器无法启动

如果看到错误 `spawn /path/to/mutica-lsp ENOENT`：

1. **确认服务器已构建**：
   ```bash
   cargo build --release
   ls -la target/release/mutica-lsp
   ```

2. **检查工作区**：确保 VS Code 打开的是正确的工作区目录

3. **手动配置路径**：在设置中明确指定服务器路径

4. **检查权限**：确保二进制文件有执行权限
   ```bash
   chmod +x target/release/mutica-lsp
   ```

5. **查看日志**：启用详细日志以诊断问题
   ```json
   {
     "muticaLsp.trace.server": "verbose"
   }
   ```

### 扩展未激活

确保你的文件扩展名是 `.mu`，这是触发扩展激活的条件。

## 开发

### LSP 服务器 (Rust)

LSP 服务器位于 `src/main.rs`，使用 tower-lsp 框架实现。主要功能包括：

- `initialize`: 初始化服务器能力
- `did_open/did_change/did_save/did_close`: 文档生命周期事件
- `completion`: 代码补全
- `execute_command`: 命令执行

### VS Code 扩展 (TypeScript)

扩展代码位于 `src-vscode/extension.ts`，负责：

- 启动和管理 LSP 服务器进程
- 配置文档选择器和文件监视
- 处理扩展的激活和停用

### 语法高亮

语法定义在 `syntaxes/mutica.tmGrammar.json`，支持：

- 关键词高亮
- 字符串和注释
- 数字字面量

## 扩展功能

你可以根据需要扩展以下功能：

1. **更多 LSP 功能**：
   - 代码诊断 (diagnostics)
   - 跳转到定义 (go to definition)
   - 查找引用 (find references)
   - 重命名 (rename)
   - 代码格式化 (formatting)

2. **语法解析**：
   - 集成实际的 mutica 语言解析器
   - 提供语义分析

3. **调试支持**：
   - 实现调试适配器协议 (DAP)

## 依赖

### Rust 依赖
- `tower-lsp`: LSP 框架
- `tokio`: 异步运行时
- `serde_json`: JSON 序列化
- `anyhow`: 错误处理

### Node.js 依赖
- `vscode-languageclient`: VS Code 语言客户端库
- `typescript`: TypeScript 编译器

## 许可证

[添加你的许可证信息]
