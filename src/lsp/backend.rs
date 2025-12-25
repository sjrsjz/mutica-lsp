use std::collections::{HashMap, HashSet};
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
    pub reference_table: RwLock<HashMap<Url, Vec<(Range, Location)>>>,
    pub variable_maps: RwLock<HashMap<Url, Vec<Option<Vec<String>>>>>,
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
        // chdir to uri's parent directory
        std::env::set_current_dir(
            uri.to_file_path()
                .unwrap()
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )
        .unwrap_or(());
        if let Some(change) = params.content_changes.first() {
            self.documents
                .write()
                .unwrap()
                .insert(uri.clone(), change.text.clone());

            // 先尝试解析，只有成功时才触发 semantic_tokens_refresh
            let result = parse_and_generate_tokens(&change.text, &uri, &self.client).await;
            if let Ok((Some(tokens), reference_table, variable_map_opt)) = result {
                // 解析成功，缓存 tokens 和引用表
                self.last_tokens
                    .write()
                    .unwrap()
                    .insert(uri.clone(), tokens);
                self.reference_table
                    .write()
                    .unwrap()
                    .insert(uri.clone(), reference_table);
                if let Some(variable_map) = variable_map_opt {
                    self.variable_maps
                        .write()
                        .unwrap()
                        .insert(uri.clone(), variable_map);
                }
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

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let mut items = crate::lsp::completion::get_completion_items();

        // 获取位置信息
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        // 从变量映射获取变量补全
        if let Some(variable_items) = crate::lsp::completion::get_variable_completions(
            &uri,
            position,
            &self.documents,
            &self.variable_maps,
        ) {
            items.extend(variable_items);
        }

        Ok(Some(CompletionResponse::Array(items)))
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
        std::env::set_current_dir(
            uri.to_file_path()
                .unwrap()
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )
        .unwrap_or(());
        let content = self.documents.read().unwrap().get(&uri).cloned();
        if let Some(content) = content {
            let result = parse_and_generate_tokens(&content, &uri, &self.client).await?;
            if let (Some(tokens), reference_table, variable_map_opt) = result {
                self.last_tokens
                    .write()
                    .unwrap()
                    .insert(uri.clone(), tokens.clone());
                self.reference_table
                    .write()
                    .unwrap()
                    .insert(uri.clone(), reference_table);
                if let Some(variable_map) = variable_map_opt {
                    self.variable_maps
                        .write()
                        .unwrap()
                        .insert(uri.clone(), variable_map);
                }
                Ok(Some(SemanticTokensResult::Tokens(tokens)))
            } else if let Some(cached) = self.last_tokens.read().unwrap().get(&uri).cloned() {
                Ok(Some(SemanticTokensResult::Tokens(cached)))
            } else {
                Ok(None)
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
            for (use_range, def_location) in references {
                if position_in_range(&position, use_range) {
                    return Ok(Some(GotoDefinitionResponse::Scalar(def_location.clone())));
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

        // 首先找到目标定义的位置
        let mut target_def_location: Option<Location> = None;

        // 检查当前文件中是否有使用指向某个定义
        if let Some(references) = table.get(&uri) {
            for (use_range, def_location) in references {
                if position_in_range(&position, use_range) {
                    target_def_location = Some(def_location.clone());
                    break;
                }
            }

            // 如果没找到，检查光标是否在定义位置
            if target_def_location.is_none() {
                for (_, def_location) in references {
                    if def_location.uri == uri && position_in_range(&position, &def_location.range)
                    {
                        target_def_location = Some(def_location.clone());
                        break;
                    }
                }
            }
        }

        // 如果在当前文件没找到，检查其他文件的定义位置
        if target_def_location.is_none() {
            for (_, references) in table.iter() {
                for (_, def_location) in references {
                    if def_location.uri == uri && position_in_range(&position, &def_location.range)
                    {
                        target_def_location = Some(def_location.clone());
                        break;
                    }
                }
                if target_def_location.is_some() {
                    break;
                }
            }
        }

        if let Some(def_location) = target_def_location {
            let mut locations = Vec::new();

            // 遍历所有文件查找指向该定义的引用
            for (file_uri, references) in table.iter() {
                for (use_range, d_location) in references {
                    if d_location.uri == def_location.uri
                        && ranges_equal(&d_location.range, &def_location.range)
                    {
                        locations.push(Location {
                            uri: file_uri.clone(),
                            range: *use_range,
                        });
                    }
                }
            }

            if include_declaration {
                locations.push(def_location);
            }

            return Ok(Some(locations));
        }

        Ok(None)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        let table = self.reference_table.read().unwrap();

        // 首先找到目标定义的位置
        let mut target_def_location: Option<Location> = None;

        // 检查当前文件中是否有使用指向某个定义
        if let Some(references) = table.get(&uri) {
            for (use_range, def_location) in references {
                if position_in_range(&position, use_range) {
                    target_def_location = Some(def_location.clone());
                    break;
                }
            }

            // 如果没找到，检查光标是否在定义位置
            if target_def_location.is_none() {
                for (_, def_location) in references {
                    if def_location.uri == uri && position_in_range(&position, &def_location.range)
                    {
                        target_def_location = Some(def_location.clone());
                        break;
                    }
                }
            }
        }

        // 如果在当前文件没找到，检查其他文件的定义位置
        if target_def_location.is_none() {
            for (_, references) in table.iter() {
                for (_, def_location) in references {
                    if def_location.uri == uri && position_in_range(&position, &def_location.range)
                    {
                        target_def_location = Some(def_location.clone());
                        break;
                    }
                }
                if target_def_location.is_some() {
                    break;
                }
            }
        }

        if let Some(def_location) = target_def_location {
            // URI 规范化：转成文件路径再比较
            let def_path = def_location.uri.to_file_path().ok();
            
            // 直接在 changes 层面去重
            let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
            let mut processed: HashSet<(String, u32, u32, u32, u32)> = HashSet::new();
            
            // URI 规范化映射
            let mut uri_map: HashMap<String, Url> = HashMap::new();
            let mut get_canonical_uri = |uri: &Url| -> Url {
                if let Ok(path) = uri.to_file_path() {
                    let key = path.to_string_lossy().to_lowercase();
                    uri_map.entry(key).or_insert_with(|| uri.clone()).clone()
                } else {
                    uri.clone()
                }
            };

            // 辅助函数：添加编辑操作（带去重）
            let mut add_edit = |uri: &Url, range: Range| {
                let key = if let Ok(path) = uri.to_file_path() {
                    (
                        path.to_string_lossy().to_lowercase(),
                        range.start.line,
                        range.start.character,
                        range.end.line,
                        range.end.character,
                    )
                } else {
                    (
                        uri.to_string().to_lowercase(),
                        range.start.line,
                        range.start.character,
                        range.end.line,
                        range.end.character,
                    )
                };
                
                if processed.insert(key) {
                    let canonical = get_canonical_uri(uri);
                    changes.entry(canonical).or_default().push(TextEdit {
                        range,
                        new_text: new_name.clone(),
                    });
                }
            };

            // 1. 添加定义位置的编辑
            add_edit(&def_location.uri, def_location.range);

            // 2. 添加所有引用位置的编辑
            for (file_uri, references) in table.iter() {
                for (use_range, d_location) in references {
                    // 用文件路径比较，不用 URI 字符串
                    let d_path = d_location.uri.to_file_path().ok();
                    if d_path.is_some() && d_path == def_path
                        && ranges_equal(&d_location.range, &def_location.range)
                    {
                        add_edit(file_uri, *use_range);
                    }
                }
            }

            return Ok(Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }));
        }

        Ok(None)
    }
}
