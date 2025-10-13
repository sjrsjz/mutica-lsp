import * as path from 'path';
import * as fs from 'fs';
import { workspace, ExtensionContext, window, commands } from 'vscode';

import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind
} from 'vscode-languageclient/node';

let client: LanguageClient;
let muticaTerminal: any | undefined;

/**
 * 获取可执行文件的平台特定名称
 * 在 Windows 上添加 .exe 扩展名
 */
function getExecutableName(baseName: string): string {
    return process.platform === 'win32' ? `${baseName}.exe` : baseName;
}

/**
 * 获取 LSP 服务器的路径
 */
function getServerPath(context: ExtensionContext): string {
    const execName = getExecutableName('mutica-lsp');
    return path.join(context.extensionPath, 'bin', execName);
}

/**
 * 获取 Mutica 编译器的路径（从 PATH 或 cargo 安装位置）
 */
function getCompilerPath(): string | undefined {
    const execName = getExecutableName('mutica');
    const homeDir = process.env.HOME || process.env.USERPROFILE || '';
    
    // 尝试从 cargo 安装位置查找
    const cargoPath = path.join(homeDir, '.cargo', 'bin', execName);
    if (fs.existsSync(cargoPath)) {
        return cargoPath;
    }
    
    // 假设在 PATH 中
    return execName;
}

export function activate(context: ExtensionContext) {
    // 获取服务器可执行文件路径（打包在扩展中）
    const serverPath = getServerPath(context);

    // 检查文件是否存在
    if (!fs.existsSync(serverPath)) {
        window.showErrorMessage(
            'Mutica LSP server not found in extension bundle. ' +
            'Please reinstall the extension or report this issue.'
        );
        return;
    }

    // 输出日志用于调试
    console.log(`Mutica LSP: Using server path: ${serverPath}`);

    // 服务器选项
    const serverOptions: ServerOptions = {
        run: { command: serverPath, transport: TransportKind.stdio },
        debug: { command: serverPath, transport: TransportKind.stdio }
    };

    // 客户端选项
    const clientOptions: LanguageClientOptions = {
        // 注册服务器为 mutica 文档
        documentSelector: [{ scheme: 'file', language: 'mutica' }],
        synchronize: {
            // 通知服务器工作区文件夹中 .mutica 文件的变化
            fileEvents: workspace.createFileSystemWatcher('**/.mu')
        }
    };

    // 注册运行命令
    context.subscriptions.push(commands.registerCommand('mutica.run', async () => {
        const activeEditor = window.activeTextEditor;
        if (activeEditor && activeEditor.document.languageId === 'mutica') {
            // 自动保存当前文件
            await activeEditor.document.save();

            const filePath = activeEditor.document.uri.fsPath;
            const compilerPath = getCompilerPath();
            if (compilerPath) {
                // 复用或创建终端
                if (!muticaTerminal || muticaTerminal.exitStatus !== undefined) {
                    muticaTerminal = window.createTerminal('Mutica Run');
                }

                // 构建命令：如果编译器路径包含空格或路径分隔符，需要引号
                const needsQuotes = compilerPath.includes(' ') && (compilerPath.includes(path.sep) || compilerPath.includes('/'));
                const quotedCompiler = needsQuotes ? `"${compilerPath}"` : compilerPath;
                const command = `${quotedCompiler} run "${filePath}"`;
                muticaTerminal.sendText(command);
                muticaTerminal.show();
            } else {
                window.showErrorMessage(
                    'Mutica compiler not found. Please install it using "cargo install --path ." ' +
                    'or ensure it is in your PATH.'
                );
            }
        } else {
            window.showWarningMessage('No active Mutica file.');
        }
    }));

    // 监听终端关闭事件
    context.subscriptions.push(window.onDidCloseTerminal(terminal => {
        if (terminal === muticaTerminal) {
            muticaTerminal = undefined;
        }
    }));

    // 创建语言客户端并启动
    client = new LanguageClient(
        'muticaLsp',
        'Mutica Language Server',
        serverOptions,
        clientOptions
    );

    // 启动客户端。这也会启动服务器
    client.start();
}

export function deactivate(): Thenable<void> | undefined {
    if (!client) {
        return undefined;
    }
    return client.stop();
}
