use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::lsp::semantic::parse_and_generate_tokens;
use crate::lsp::utils::{position_in_range, ranges_equal};

#[derive(Debug)]
pub struct Backend {
    pub client: Client,
    pub documents: RwLock<HashMap<Url, String>>,
    pub last_tokens: RwLock<HashMap<Url, SemanticTokens>>,
    pub reference_table: RwLock<HashMap<Url, Vec<(Range, Range)>>>,
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
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: Default::default(),
                            legend: SemanticTokensLegend {
                                token_types: vec![
                                    SemanticTokenType::NAMESPACE,
                                    SemanticTokenType::TYPE,
                                    SemanticTokenType::CLASS,
                                    SemanticTokenType::ENUM,
                                    SemanticTokenType::INTERFACE,
                                    SemanticTokenType::STRUCT,
                                    SemanticTokenType::TYPE_PARAMETER,
                                    SemanticTokenType::PARAMETER,
                                    SemanticTokenType::VARIABLE,
                                    SemanticTokenType::PROPERTY,
                                    SemanticTokenType::ENUM_MEMBER,
                                    SemanticTokenType::EVENT,
                                    SemanticTokenType::FUNCTION,
                                    SemanticTokenType::METHOD,
                                    SemanticTokenType::MACRO,
                                    SemanticTokenType::KEYWORD,
                                    SemanticTokenType::MODIFIER,
                                    SemanticTokenType::COMMENT,
                                    SemanticTokenType::STRING,
                                    SemanticTokenType::NUMBER,
                                    SemanticTokenType::REGEXP,
                                    SemanticTokenType::OPERATOR,
                                ],
                                token_modifiers: vec![
                                    SemanticTokenModifier::DECLARATION,
                                    SemanticTokenModifier::DEFINITION,
                                    SemanticTokenModifier::READONLY,
                                    SemanticTokenModifier::STATIC,
                                    SemanticTokenModifier::DEPRECATED,
                                    SemanticTokenModifier::ABSTRACT,
                                    SemanticTokenModifier::ASYNC,
                                    SemanticTokenModifier::MODIFICATION,
                                    SemanticTokenModifier::DOCUMENTATION,
                                    SemanticTokenModifier::DEFAULT_LIBRARY,
                                ],
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

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.first() {
            self.documents
                .write()
                .unwrap()
                .insert(uri.clone(), change.text.clone());

            // 先尝试解析，只有成功时才触发 semantic_tokens_refresh
            let result = parse_and_generate_tokens(&change.text, &uri, &self.client).await;
            if let Ok((Some(tokens), reference_table)) = result {
                // 解析成功，缓存 tokens 和引用表
                self.last_tokens
                    .write()
                    .unwrap()
                    .insert(uri.clone(), tokens);
                self.reference_table
                    .write()
                    .unwrap()
                    .insert(uri.clone(), reference_table);
                let _ = self.client.semantic_tokens_refresh().await;
            }
        }

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
        let content = self.documents.read().unwrap().get(&uri).cloned();
        if let Some(content) = content {
            let result = parse_and_generate_tokens(&content, &uri, &self.client).await?;
            if let (Some(tokens), reference_table) = result {
                self.last_tokens
                    .write()
                    .unwrap()
                    .insert(uri.clone(), tokens.clone());
                self.reference_table
                    .write()
                    .unwrap()
                    .insert(uri, reference_table);
                Ok(Some(SemanticTokensResult::Tokens(tokens)))
            } else {
                if let Some(cached) = self.last_tokens.read().unwrap().get(&uri).cloned() {
                    Ok(Some(SemanticTokensResult::Tokens(cached)))
                } else {
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let table = self.reference_table.read().unwrap();
        if let Some(references) = table.get(&uri) {
            for (use_range, def_range) in references {
                if position_in_range(&position, use_range) {
                    return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                        uri: uri.clone(),
                        range: *def_range,
                    })));
                }
            }
        }
        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        let table = self.reference_table.read().unwrap();
        if let Some(references) = table.get(&uri) {
            let mut target_def_range: Option<Range> = None;

            for (use_range, def_range) in references {
                if position_in_range(&position, use_range) {
                    target_def_range = Some(*def_range);
                    break;
                }
            }

            if target_def_range.is_none() {
                for (_, def_range) in references {
                    if position_in_range(&position, def_range) {
                        target_def_range = Some(*def_range);
                        break;
                    }
                }
            }

            if let Some(def_range) = target_def_range {
                let mut locations = Vec::new();

                for (use_range, d_range) in references {
                    if ranges_equal(d_range, &def_range) {
                        locations.push(Location {
                            uri: uri.clone(),
                            range: *use_range,
                        });
                    }
                }

                if include_declaration {
                    locations.push(Location {
                        uri: uri.clone(),
                        range: def_range,
                    });
                }

                return Ok(Some(locations));
            }
        }
        Ok(None)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        let table = self.reference_table.read().unwrap();
        if let Some(references) = table.get(&uri) {
            let mut target_def_range: Option<Range> = None;

            for (use_range, def_range) in references {
                if position_in_range(&position, use_range) {
                    target_def_range = Some(*def_range);
                    break;
                }
            }

            if target_def_range.is_none() {
                for (_, def_range) in references {
                    if position_in_range(&position, def_range) {
                        target_def_range = Some(*def_range);
                        break;
                    }
                }
            }

            if let Some(def_range) = target_def_range {
                let mut text_edits = Vec::new();

                text_edits.push(TextEdit {
                    range: def_range,
                    new_text: new_name.clone(),
                });

                for (use_range, d_range) in references {
                    if ranges_equal(d_range, &def_range) {
                        text_edits.push(TextEdit {
                            range: *use_range,
                            new_text: new_name.clone(),
                        });
                    }
                }

                let mut changes = HashMap::new();
                changes.insert(uri.clone(), text_edits);

                return Ok(Some(WorkspaceEdit {
                    changes: Some(changes),
                    document_changes: None,
                    change_annotations: None,
                }));
            }
        }
        Ok(None)
    }
}
