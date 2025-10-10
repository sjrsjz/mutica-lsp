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

function findServerPath(context: ExtensionContext): string | undefined {
    const config = workspace.getConfiguration('muticaLsp');
    let serverPath = config.get<string>('serverPath');

    // 如果用户配置了路径
    if (serverPath) {
        // 如果包含路径分隔符，检查文件是否存在
        if (serverPath.includes(path.sep) || serverPath.includes('/')) {
            if (fs.existsSync(serverPath)) {
                return serverPath;
            } else {
                window.showWarningMessage(`Configured Mutica LSP server path does not exist: ${serverPath}`);
            }
        } else {
            // 假设是命令名，直接返回
            return serverPath;
        }
    }

    // 尝试多个可能的位置
    const possiblePaths = [
        // 1. 扩展安装目录中
        path.join(context.extensionPath, 'target', 'release', 'mutica-lsp'),
        path.join(context.extensionPath, 'target', 'debug', 'mutica-lsp'),

        // 2. 工作区根目录中
        ...(workspace.workspaceFolders?.map(folder =>
            path.join(folder.uri.fsPath, 'target', 'release', 'mutica-lsp')
        ) || []),
        ...(workspace.workspaceFolders?.map(folder =>
            path.join(folder.uri.fsPath, 'target', 'debug', 'mutica-lsp')
        ) || []),

        // 3. 假设在PATH中
        'mutica-lsp'
    ];

    // 查找第一个存在的路径，或返回'mutica-lsp'如果在PATH中
    for (const p of possiblePaths) {
        if (p === 'mutica-lsp' || fs.existsSync(p)) {
            return p;
        }
    }

    return undefined;
}

function findCompilerPath(context: ExtensionContext): string | undefined {
    const config = workspace.getConfiguration('muticaLsp');
    let compilerPath = config.get<string>('compilerPath');

    // 如果用户配置了路径
    if (compilerPath) {
        // 如果包含路径分隔符，检查文件是否存在
        if (compilerPath.includes(path.sep) || compilerPath.includes('/')) {
            if (fs.existsSync(compilerPath)) {
                return compilerPath;
            } else {
                window.showWarningMessage(`Configured Mutica compiler path does not exist: ${compilerPath}`);
            }
        } else {
            // 假设是命令名，直接返回
            return compilerPath;
        }
    }

    // 尝试多个可能的位置
    const possiblePaths = [
        // 1. 扩展安装目录的兄弟目录中
        path.join(context.extensionPath, '..', 'Mutica', 'target', 'release', 'mutica'),
        path.join(context.extensionPath, '..', 'Mutica', 'target', 'debug', 'mutica'),

        // 2. 工作区根目录中
        ...(workspace.workspaceFolders?.map(folder =>
            path.join(folder.uri.fsPath, 'target', 'release', 'mutica')
        ) || []),
        ...(workspace.workspaceFolders?.map(folder =>
            path.join(folder.uri.fsPath, 'target', 'debug', 'mutica')
        ) || []),

        // 3. 假设在PATH中
        'mutica'
    ];

    // 查找第一个存在的路径，或返回'mutica'如果在PATH中
    for (const p of possiblePaths) {
        if (p === 'mutica' || fs.existsSync(p)) {
            return p;
        }
    }

    return undefined;
}

export function activate(context: ExtensionContext) {
    // 查找服务器可执行文件路径
    const serverPath = findServerPath(context);

    if (!serverPath) {
        window.showErrorMessage(
            'Mutica LSP server not found. Please build it using "cargo build --release" ' +
            'or configure the path in settings (muticaLsp.serverPath).'
        );
        return;
    }

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
            const compilerPath = findCompilerPath(context);
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
                    'Mutica compiler not found. Please configure the path in settings (muticaLsp.compilerPath).'
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
