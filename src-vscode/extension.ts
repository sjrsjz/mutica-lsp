import * as path from 'path';
import * as fs from 'fs';
import { workspace, ExtensionContext, window } from 'vscode';

import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind
} from 'vscode-languageclient/node';

let client: LanguageClient;

function findServerPath(context: ExtensionContext): string | undefined {
    const config = workspace.getConfiguration('muticaLsp');
    let serverPath = config.get<string>('serverPath');
    
    // 如果用户配置了路径，验证它是否存在
    if (serverPath) {
        if (fs.existsSync(serverPath)) {
            return serverPath;
        } else {
            window.showWarningMessage(`Configured Mutica LSP server path does not exist: ${serverPath}`);
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
    ];
    
    // 查找第一个存在的路径
    for (const p of possiblePaths) {
        if (fs.existsSync(p)) {
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
