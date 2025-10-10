use mutica::mutica_compiler::parser::{SourceFile, ast::LinearTypeAst};
use mutica::mutica_compiler::{
    grammar::TypeParser,
    logos::Logos,
    parser::{ParseContext, ast::LinearizeContext, lexer::LexerToken},
};
use mutica::mutica_semantic::semantic::SourceMapping;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: RwLock<HashMap<Url, String>>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: None,
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string()]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: None,
                }),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["dummy.do_something".to_string()],
                    work_done_progress_options: Default::default(),
                }),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: Default::default(),
                            legend: SemanticTokensLegend {
                                token_types: vec![
                                    SemanticTokenType::VARIABLE,
                                    SemanticTokenType::FUNCTION,
                                    SemanticTokenType::TYPE,
                                    SemanticTokenType::KEYWORD,
                                    SemanticTokenType::STRING,
                                    SemanticTokenType::NUMBER,
                                    SemanticTokenType::COMMENT,
                                ],
                                token_modifiers: vec![],
                            },
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Mutica LSP server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.documents
            .write()
            .unwrap()
            .insert(params.text_document.uri, params.text_document.text);
        self.client
            .log_message(MessageType::INFO, "file opened!")
            .await;
    }

    async fn did_change(&self, _: DidChangeTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "file changed!")
            .await;
    }

    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "file saved!")
            .await;
    }

    async fn did_close(&self, _: DidCloseTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "file closed!")
            .await;
    }

    async fn completion(&self, _: CompletionParams) -> Result<Option<CompletionResponse>> {
        Ok(Some(CompletionResponse::Array(vec![
            CompletionItem::new_simple("Hello".to_string(), "Some details".to_string()),
            CompletionItem::new_simple("Bye".to_string(), "More details".to_string()),
        ])))
    }

    async fn execute_command(&self, _: ExecuteCommandParams) -> Result<Option<serde_json::Value>> {
        self.client
            .log_message(MessageType::INFO, "command executed!")
            .await;

        match self.client.apply_edit(WorkspaceEdit::default()).await {
            Ok(res) if res.applied => self.client.log_message(MessageType::INFO, "applied").await,
            Ok(_) => self.client.log_message(MessageType::INFO, "rejected").await,
            Err(err) => self.client.log_message(MessageType::ERROR, err).await,
        }

        Ok(None)
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        if let Some(content) = self.documents.read().unwrap().get(&uri) {
            let tokens = self.parse_and_generate_tokens(content)?;
            Ok(Some(SemanticTokensResult::Tokens(tokens)))
        } else {
            Ok(None)
        }
    }
}

impl Backend {
    fn parse_and_generate_tokens(&self, content: &str) -> Result<SemanticTokens> {
        let source_file = Arc::new(SourceFile::new(None, content.into()));
        let lexer = LexerToken::lexer(content);
        let spanned_lexer = lexer.spanned().map(|(token_result, span)| {
            let token = token_result?;
            Ok((span.start, token, span.end))
        });
        let parser = TypeParser::new();
        let parsed = parser.parse(&source_file, spanned_lexer);
        match parsed {
            Ok(ast) => {
                let mut errors = Vec::new();
                ast.collect_errors(&mut errors);
                if !errors.is_empty() {
                    return Ok(SemanticTokens {
                        result_id: None,
                        data: vec![],
                    });
                }
                let basic = ast.into_basic(ast.location());
                let linearized = basic
                    .linearize(&mut LinearizeContext::new(), basic.location())
                    .finalize();
                let flow_result =
                    linearized.flow(&mut ParseContext::new(), false, linearized.location());
                let flowed = match &flow_result {
                    Ok(result) => result.ty().clone(),
                    Err(_) => {
                        return Ok(SemanticTokens {
                            result_id: None,
                            data: vec![],
                        });
                    }
                };
                let mapping = SourceMapping::from_ast(&flowed, &source_file);
                // 生成tokens
                let mut tokens = Vec::new();
                let mut last_line = 0u32;
                let mut last_start = 0u32;
                let mut current_start: Option<usize> = None;
                let mut current_type: Option<u32> = None;
                for (i, node_opt) in mapping.mapping().iter().enumerate() {
                    let ty = if let Some(node) = node_opt {
                        self.ast_node_to_token_type(&node.value())
                    } else {
                        6 // COMMENT
                    };
                    if current_type != Some(ty) {
                        if let (Some(start), Some(typ)) = (current_start, current_type) {
                            // 输出token
                            let length = i - start;
                            // 计算行和列
                            let before = &content[..start];
                            let lines: Vec<&str> = before.split('\n').collect();
                            let line = lines.len() - 1;
                            let col = lines.last().unwrap().len();
                            let delta_line = line as u32 - last_line;
                            let delta_start = if delta_line == 0 {
                                col as u32 - last_start
                            } else {
                                col as u32
                            };
                            tokens.push(SemanticToken {
                                delta_line,
                                delta_start,
                                length: length as u32,
                                token_type: typ,
                                token_modifiers_bitset: 0,
                            });
                            last_line = line as u32;
                            last_start = col as u32;
                        }
                        current_start = Some(i);
                        current_type = Some(ty);
                    }
                }
                // 输出最后一个token
                if let (Some(start), Some(typ)) = (current_start, current_type) {
                    let length = content.len() - start;
                    let before = &content[..start];
                    let lines: Vec<&str> = before.split('\n').collect();
                    let line = lines.len() - 1;
                    let col = lines.last().unwrap().len();
                    let delta_line = line as u32 - last_line;
                    let delta_start = if delta_line == 0 {
                        col as u32 - last_start
                    } else {
                        col as u32
                    };
                    tokens.push(SemanticToken {
                        delta_line,
                        delta_start,
                        length: length as u32,
                        token_type: typ,
                        token_modifiers_bitset: 0,
                    });
                }
                Ok(SemanticTokens {
                    result_id: None,
                    data: tokens,
                })
            }
            Err(_) => Ok(SemanticTokens {
                result_id: None,
                data: vec![],
            }),
        }
    }

    fn ast_node_to_token_type(&self, node: &LinearTypeAst) -> u32 {
        match node {
            LinearTypeAst::Variable(_) => 0,      // VARIABLE
            LinearTypeAst::Closure { .. } => 1,   // FUNCTION
            LinearTypeAst::Invoke { .. } => 1,    // FUNCTION
            LinearTypeAst::FixPoint { .. } => 1,  // FUNCTION
            LinearTypeAst::Int => 2,              // TYPE
            LinearTypeAst::Char => 2,             // TYPE
            LinearTypeAst::Top => 2,              // TYPE
            LinearTypeAst::Bottom => 2,           // TYPE
            LinearTypeAst::Tuple(_) => 2,         // TYPE
            LinearTypeAst::List(_) => 2,          // TYPE
            LinearTypeAst::Generalize(_) => 2,    // TYPE
            LinearTypeAst::Specialize(_) => 2,    // TYPE
            LinearTypeAst::Namespace { .. } => 2, // TYPE
            LinearTypeAst::IntLiteral(_) => 5,    // NUMBER
            LinearTypeAst::CharLiteral(_) => 4,   // STRING
            LinearTypeAst::Literal(_) => 4,       // STRING
            LinearTypeAst::AtomicOpcode(_) => 3,  // KEYWORD
            LinearTypeAst::Pattern { .. } => 0,   // VARIABLE
        }
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: RwLock::new(HashMap::new()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
