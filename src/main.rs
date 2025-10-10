use mutica::mutica_compiler::SyntaxError;
use mutica::mutica_compiler::parser::{
    ParseContext, ParseError, calculate_full_error_span, report_error_recovery,
};
use mutica::mutica_compiler::parser::{
    SourceFile, WithLocation, ast::FlowedMetaData, ast::LinearTypeAst, ast::TypeAst,
};
use mutica::mutica_compiler::{
    grammar::TypeParser,
    logos::Logos,
    parser::{ast::LinearizeContext, lexer::LexerToken},
};
use mutica::mutica_semantic::semantic::SourceMapping;
use std::collections::HashMap;
use std::fmt::Debug;
use std::io::Write;
use std::sync::{Arc, RwLock};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

// Strip ANSI CSI sequences from a string. Covers common ESC '[' ... final-byte sequences.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some(next) = chars.next() {
                if next == '[' {
                    // consume until a final byte in range '@'..='~'
                    while let Some(n) = chars.next() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

// Generic helper: write a report into an in-memory buffer via the provided closure,
// strip ANSI codes and return the plain text string.
fn report_to_plain_text<F>(write_report: F) -> String
where
    F: FnOnce(&mut Vec<u8>) -> std::io::Result<()>,
{
    let mut buf: Vec<u8> = Vec::new();
    let _ = write_report(&mut buf);
    let out = String::from_utf8_lossy(&buf);
    strip_ansi(&out)
}

// 将字节偏移转换为行列号
fn offset_to_position(content: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    let mut current_offset = 0;

    for ch in content.chars() {
        if current_offset >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
        current_offset += ch.len_utf8();
    }

    Position {
        line,
        character: col,
    }
}

// 递归清理 AST 中的 ParseError 节点，将其替换为 Bottom
fn sanitize_ast<'input>(ast: WithLocation<TypeAst<'input>>) -> WithLocation<TypeAst<'input>> {
    ast.map(|value| match value {
        TypeAst::ParseError(_) => TypeAst::Bottom,
        TypeAst::Tuple(items) => TypeAst::Tuple(items.into_iter().map(sanitize_ast).collect()),
        TypeAst::List(items) => TypeAst::List(items.into_iter().map(sanitize_ast).collect()),
        TypeAst::Generalize(items) => {
            TypeAst::Generalize(items.into_iter().map(sanitize_ast).collect())
        }
        TypeAst::Specialize(items) => {
            TypeAst::Specialize(items.into_iter().map(sanitize_ast).collect())
        }
        TypeAst::Invoke {
            func,
            arg,
            continuation,
        } => TypeAst::Invoke {
            func: Box::new(sanitize_ast(*func)),
            arg: Box::new(sanitize_ast(*arg)),
            continuation: Box::new(sanitize_ast(*continuation)),
        },
        TypeAst::Expression {
            binding_patterns,
            binding_types,
            body,
        } => TypeAst::Expression {
            binding_patterns: binding_patterns.into_iter().map(sanitize_ast).collect(),
            binding_types: binding_types.into_iter().map(sanitize_ast).collect(),
            body: Box::new(sanitize_ast(*body)),
        },
        TypeAst::Match {
            value,
            match_branch,
            else_branch,
        } => TypeAst::Match {
            value: Box::new(sanitize_ast(*value)),
            match_branch: match_branch
                .into_iter()
                .map(|(pattern, expr)| (sanitize_ast(pattern), sanitize_ast(expr)))
                .collect(),
            else_branch: else_branch.map(|b| Box::new(sanitize_ast(*b))),
        },
        TypeAst::Closure {
            pattern,
            body,
            fail_branch,
        } => TypeAst::Closure {
            pattern: Box::new(sanitize_ast(*pattern)),
            body: Box::new(sanitize_ast(*body)),
            fail_branch: fail_branch.map(|b| Box::new(sanitize_ast(*b))),
        },
        TypeAst::Apply { func, arg } => TypeAst::Apply {
            func: Box::new(sanitize_ast(*func)),
            arg: Box::new(sanitize_ast(*arg)),
        },
        TypeAst::Eq { left, right } => TypeAst::Eq {
            left: Box::new(sanitize_ast(*left)),
            right: Box::new(sanitize_ast(*right)),
        },
        TypeAst::Neq { left, right } => TypeAst::Neq {
            left: Box::new(sanitize_ast(*left)),
            right: Box::new(sanitize_ast(*right)),
        },
        TypeAst::Not { value } => TypeAst::Not {
            value: Box::new(sanitize_ast(*value)),
        },
        TypeAst::FixPoint { param_name, expr } => TypeAst::FixPoint {
            param_name,
            expr: Box::new(sanitize_ast(*expr)),
        },
        TypeAst::Namespace { tag, expr } => TypeAst::Namespace {
            tag,
            expr: Box::new(sanitize_ast(*expr)),
        },
        TypeAst::Pattern { name, expr } => TypeAst::Pattern {
            name,
            expr: Box::new(sanitize_ast(*expr)),
        },
        TypeAst::Literal(inner) => TypeAst::Literal(Box::new(sanitize_ast(*inner))),
        // 基础类型保持不变
        other => other,
    })
}

// 将 ParseError 转换为友好的单行消息
fn perr_to_message(err: &ParseError) -> Option<String> {
    match err {
        ParseError::UseBeforeDeclaration(_, name) => {
            Some(format!("Use of undeclared variable '{}'", name))
        }
        ParseError::RedeclaredPattern(_, name) => {
            Some(format!("Redeclared pattern variable '{}'", name.value()))
        }
        ParseError::UnusedVariable(_, names) => {
            let vars: Vec<String> = names.iter().map(|n| n.value().clone()).collect();
            Some(format!("Unused variables: {}", vars.join(", ")))
        }
        ParseError::AmbiguousPattern(_) => Some("Ambiguous pattern".to_string()),
        ParseError::PatternOutOfParameterDefinition(_) => {
            Some("Pattern out of parameter definition".to_string())
        }
        ParseError::MissingBranch(_) => Some("Missing required branch".to_string()),
        ParseError::InternalError(msg) => Some(format!("Internal error: {}", msg)),
    }
}

#[derive(Debug)]
struct Backend {
    client: Client,
    documents: RwLock<HashMap<Url, String>>,
    // 缓存每个文件上次成功生成的 semantic tokens
    last_tokens: RwLock<HashMap<Url, SemanticTokens>>,
    // 缓存每个文件的引用表：Vec<(使用位置, 定义位置)>
    reference_table: RwLock<HashMap<Url, Vec<(Range, Range)>>>,
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
                                // Expanded, ordered list of token types. The numeric
                                // indices returned by ast_node_to_token_type must
                                // match this order.
                                token_types: vec![
                                    SemanticTokenType::NAMESPACE,      // 0
                                    SemanticTokenType::TYPE,           // 1
                                    SemanticTokenType::CLASS,          // 2
                                    SemanticTokenType::ENUM,           // 3
                                    SemanticTokenType::INTERFACE,      // 4
                                    SemanticTokenType::STRUCT,         // 5
                                    SemanticTokenType::TYPE_PARAMETER, // 6
                                    SemanticTokenType::PARAMETER,      // 7
                                    SemanticTokenType::VARIABLE,       // 8
                                    SemanticTokenType::PROPERTY,       // 9
                                    SemanticTokenType::ENUM_MEMBER,    // 10
                                    SemanticTokenType::EVENT,          // 11
                                    SemanticTokenType::FUNCTION,       // 12
                                    SemanticTokenType::METHOD,         // 13
                                    SemanticTokenType::MACRO,          // 14
                                    SemanticTokenType::KEYWORD,        // 15
                                    SemanticTokenType::MODIFIER,       // 16
                                    SemanticTokenType::COMMENT,        // 17
                                    SemanticTokenType::STRING,         // 18
                                    SemanticTokenType::NUMBER,         // 19
                                    SemanticTokenType::REGEXP,         // 20
                                    SemanticTokenType::OPERATOR,       // 21
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
        // 更新文档内容
        let uri = params.text_document.uri.clone();
        if let Some(change) = params.content_changes.first() {
            self.documents
                .write()
                .unwrap()
                .insert(uri.clone(), change.text.clone());

            // 先尝试解析，只有成功时才触发 semantic_tokens_refresh
            if let Ok(Some(_)) = self.parse_and_generate_tokens(&change.text, &uri).await {
                // 解析成功，通知客户端刷新语义高亮
                let _ = self.client.semantic_tokens_refresh().await;
            }
            // 解析失败时不调用 refresh，客户端保持现有高亮
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
            // parse_and_generate_tokens now returns Result<Option<SemanticTokens>>
            let tokens_opt = self.parse_and_generate_tokens(&content, &uri).await?;
            if let Some(tokens) = tokens_opt {
                // 解析成功，缓存新的 tokens
                self.last_tokens
                    .write()
                    .unwrap()
                    .insert(uri, tokens.clone());
                Ok(Some(SemanticTokensResult::Tokens(tokens)))
            } else {
                // 解析失败，尝试返回缓存的 tokens
                if let Some(cached) = self.last_tokens.read().unwrap().get(&uri).cloned() {
                    Ok(Some(SemanticTokensResult::Tokens(cached)))
                } else {
                    // 没有缓存，返回 None
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
            // 查找包含该位置的使用范围
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
            // 先确定用户点击的是定义还是使用
            let mut target_def_range: Option<Range> = None;

            // 检查是否点击在某个使用位置上
            for (use_range, def_range) in references {
                if position_in_range(&position, use_range) {
                    target_def_range = Some(*def_range);
                    break;
                }
            }

            // 如果没找到，检查是否点击在定义位置上
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

                // 收集所有指向该定义的引用
                for (use_range, d_range) in references {
                    if ranges_equal(d_range, &def_range) {
                        locations.push(Location {
                            uri: uri.clone(),
                            range: *use_range,
                        });
                    }
                }

                // 如果需要包含声明，添加定义位置
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
            // 找到目标定义
            let mut target_def_range: Option<Range> = None;

            // 检查是否点击在某个使用位置上
            for (use_range, def_range) in references {
                if position_in_range(&position, use_range) {
                    target_def_range = Some(*def_range);
                    break;
                }
            }

            // 如果没找到，检查是否点击在定义位置上
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

                // 收集定义位置
                text_edits.push(TextEdit {
                    range: def_range,
                    new_text: new_name.clone(),
                });

                // 收集所有引用位置
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

// 辅助函数：判断位置是否在范围内
fn position_in_range(pos: &Position, range: &Range) -> bool {
    if pos.line < range.start.line || pos.line > range.end.line {
        return false;
    }
    if pos.line == range.start.line && pos.character < range.start.character {
        return false;
    }
    if pos.line == range.end.line && pos.character > range.end.character {
        return false;
    }
    true
}

// 辅助函数：判断两个范围是否相等
fn ranges_equal(a: &Range, b: &Range) -> bool {
    a.start.line == b.start.line
        && a.start.character == b.start.character
        && a.end.line == b.end.line
        && a.end.character == b.end.character
}

impl Backend {
    // 返回 Result<Option<SemanticTokens>>，在语法/类型分析失败时返回 Ok(None)
    async fn parse_and_generate_tokens(
        &self,
        content: &str,
        uri: &Url,
    ) -> Result<Option<SemanticTokens>> {
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
                // 先收集错误信息，在 sanitize 之前
                let mut errors = Vec::new();
                ast.collect_errors(&mut errors);

                // 将错误转换为 LSP 诊断信息并发送给客户端
                // 无论 errors 是否为空都要调用 publish_diagnostics。
                // 发送空 diagnostics 可以清除客户端上之前的错误提示。
                let mut diagnostics = Vec::new();
                for err in &errors {
                    // 使用 calculate_full_error_span 获取错误的字节偏移范围
                    let (start_byte, end_byte) = calculate_full_error_span(err);
                    let start = offset_to_position(content, start_byte);
                    let end = offset_to_position(content, end_byte);

                    // 生成错误消息
                    let report = report_error_recovery(err, uri.as_str(), content);
                    let cache = (
                        uri.as_str(),
                        mutica::mutica_compiler::ariadne::Source::from(content),
                    );
                    let message =
                        report_to_plain_text(|buf: &mut Vec<u8>| report.write(cache, buf));

                    diagnostics.push(Diagnostic {
                        range: Range { start, end },
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: None,
                        code_description: None,
                        source: Some("mutica-lsp".to_string()),
                        message,
                        related_information: None,
                        tags: None,
                        data: None,
                    });
                }

                // 发送诊断信息到客户端（如果为空则清除旧诊断）
                self.client
                    .publish_diagnostics(uri.clone(), diagnostics, None)
                    .await;

                // 清理 AST 中的 ParseError 节点，避免 into_basic panic
                let sanitized_ast = sanitize_ast(ast);
                let basic = sanitized_ast.into_basic(sanitized_ast.location());
                let linearized = basic
                    .linearize(&mut LinearizeContext::new(), basic.location())
                    .finalize();

                let flowed_result: std::result::Result<
                    mutica::mutica_compiler::parser::ast::FlowResult<'_>,
                    ParseError<'_>,
                > = linearized.flow(&mut ParseContext::new(), false, basic.location());

                match flowed_result {
                    Ok(flowed) => {
                        // 当语义分析成功时，遍历整个 AST 树，从每个节点的 FlowedMetaData.reference
                        // 中提取"使用位置 -> 定义位置"的引用关系表。
                        let mut reference_table: Vec<(Range, Range)> = Vec::new();

                        // 递归遍历 AST 节点收集引用信息
                        fn collect_references<'ast>(
                            node: &WithLocation<LinearTypeAst<'ast>, FlowedMetaData<'ast>>,
                            content: &str,
                            table: &mut Vec<(Range, Range)>,
                        ) {
                            // 检查当前节点的 reference 字段
                            if let Some(use_loc) = node.location() {
                                if let Some(ref_with_loc) = node.payload().reference() {
                                    if let Some(def_loc) = ref_with_loc.location() {
                                        let use_span = use_loc.span();
                                        let def_span = def_loc.span();

                                        let use_range = Range {
                                            start: offset_to_position(content, use_span.start),
                                            end: offset_to_position(content, use_span.end),
                                        };

                                        // 对于定义位置是 Pattern 的情况，只定位模式名而非整个模式
                                        // 从源码中提取 name 的精确范围：从 def_span.start 开始，到第一个冒号或空格结束
                                        let def_text = &content[def_span.start..def_span.end];
                                        let name_len = if let Some(colon_pos) = def_text.find(':') {
                                            colon_pos.min(def_text.find(' ').unwrap_or(colon_pos))
                                        } else {
                                            def_text.len()
                                        };

                                        let def_range = Range {
                                            start: offset_to_position(content, def_span.start),
                                            end: offset_to_position(
                                                content,
                                                def_span.start + name_len,
                                            ),
                                        };

                                        table.push((use_range, def_range));
                                    }
                                }
                            }

                            // 递归遍历所有子节点
                            match node.value() {
                                LinearTypeAst::Tuple(items)
                                | LinearTypeAst::List(items)
                                | LinearTypeAst::Generalize(items)
                                | LinearTypeAst::Specialize(items) => {
                                    for item in items {
                                        collect_references(item, content, table);
                                    }
                                }
                                LinearTypeAst::Closure {
                                    pattern,
                                    body,
                                    fail_branch,
                                    ..
                                } => {
                                    collect_references(pattern, content, table);
                                    collect_references(body, content, table);
                                    if let Some(fb) = fail_branch {
                                        collect_references(fb, content, table);
                                    }
                                }
                                LinearTypeAst::Invoke {
                                    func,
                                    arg,
                                    continuation,
                                } => {
                                    collect_references(func, content, table);
                                    collect_references(arg, content, table);
                                    collect_references(continuation, content, table);
                                }
                                LinearTypeAst::Pattern { expr, .. } => {
                                    collect_references(expr, content, table);
                                }
                                LinearTypeAst::Namespace { expr, .. } => {
                                    collect_references(expr, content, table);
                                }
                                LinearTypeAst::FixPoint { expr, .. } => {
                                    collect_references(expr, content, table);
                                }
                                LinearTypeAst::Literal(inner) => {
                                    collect_references(inner, content, table);
                                }
                                // 叶子节点：Variable, Int, Char, Top, Bottom, IntLiteral, CharLiteral, AtomicOpcode
                                _ => {}
                            }
                        }

                        collect_references(flowed.ty(), content, &mut reference_table);

                        // 缓存引用表到 Backend
                        self.reference_table
                            .write()
                            .unwrap()
                            .insert(uri.clone(), reference_table.clone());

                        // 将引用表输出到 stderr，便于调试
                        if !reference_table.is_empty() {
                            let mut out = String::new();
                            for (use_range, def_range) in &reference_table {
                                out.push_str(&format!(
                                    "{}:{}-{}:{} -> {}:{}-{}:{}\n",
                                    use_range.start.line,
                                    use_range.start.character,
                                    use_range.end.line,
                                    use_range.end.character,
                                    def_range.start.line,
                                    def_range.start.character,
                                    def_range.end.line,
                                    def_range.end.character
                                ));
                            }
                            let _ = std::io::stderr().write_all(out.as_bytes());
                        }
                    }
                    Err(e) => {
                        // 处理语义分析错误：写入 stderr
                        let err_report = e.report();
                        let cache = (
                            uri.to_string(),
                            mutica::mutica_compiler::ariadne::Source::from(content),
                        );
                        let plain =
                            report_to_plain_text(|buf: &mut Vec<u8>| err_report.write(cache, buf));
                        let _ = std::io::stderr().write_all(plain.as_bytes());

                        // 尝试从错误中提取更精确的位置：
                        // 如果 e 包含 ParseError 或者包含 WithLocation，优先使用这些位置信息
                        let mut diagnostics: Vec<Diagnostic> = Vec::new();

                        // 下面直接匹配 ParseError 各个变体（借用），提取精确位置并生成 Diagnostic
                        match &e {
                            ParseError::UseBeforeDeclaration(ast, name) => {
                                if let Some(loc) = ast.location() {
                                    let span = loc.span().clone();
                                    let start = offset_to_position(content, span.start);
                                    let end = offset_to_position(content, span.end);
                                    let message = format!("Use of undeclared variable '{}'", name);
                                    diagnostics.push(Diagnostic {
                                        range: Range { start, end },
                                        severity: Some(DiagnosticSeverity::ERROR),
                                        code: None,
                                        code_description: None,
                                        source: Some("mutica-lsp".to_string()),
                                        message,
                                        related_information: None,
                                        tags: None,
                                        data: None,
                                    });
                                }
                            }
                            ParseError::RedeclaredPattern(ast, name) => {
                                if let Some(loc) = name.location().or_else(|| ast.location()) {
                                    let span = loc.span().clone();
                                    let start = offset_to_position(content, span.start);
                                    let end = offset_to_position(content, span.end);
                                    let message =
                                        format!("Redeclared pattern variable '{}'", name.value());
                                    diagnostics.push(Diagnostic {
                                        range: Range { start, end },
                                        severity: Some(DiagnosticSeverity::ERROR),
                                        code: None,
                                        code_description: None,
                                        source: Some("mutica-lsp".to_string()),
                                        message,
                                        related_information: None,
                                        tags: None,
                                        data: None,
                                    });
                                }
                            }
                            ParseError::UnusedVariable(_ast, names) => {
                                // 为所有有位置信息的变量生成 label
                                for name_loc in names.iter() {
                                    if let Some(loc) = name_loc.location() {
                                        let span = loc.span().clone();
                                        let start = offset_to_position(content, span.start);
                                        let end = offset_to_position(content, span.end);
                                        let message = format!(
                                            "Variable '{}' is declared but never used",
                                            name_loc.value()
                                        );
                                        diagnostics.push(Diagnostic {
                                            range: Range { start, end },
                                            severity: Some(DiagnosticSeverity::ERROR),
                                            code: None,
                                            code_description: None,
                                            source: Some("mutica-lsp".to_string()),
                                            message: message.clone(),
                                            related_information: None,
                                            tags: None,
                                            data: None,
                                        });
                                    }
                                }
                            }
                            ParseError::AmbiguousPattern(ast)
                            | ParseError::PatternOutOfParameterDefinition(ast)
                            | ParseError::MissingBranch(ast) => {
                                if let Some(loc) = ast.location() {
                                    let span = loc.span().clone();
                                    let start = offset_to_position(content, span.start);
                                    let end = offset_to_position(content, span.end);
                                    let message = match perr_to_message(&e) {
                                        Some(m) => m,
                                        None => plain.clone(),
                                    };
                                    diagnostics.push(Diagnostic {
                                        range: Range { start, end },
                                        severity: Some(DiagnosticSeverity::ERROR),
                                        code: None,
                                        code_description: None,
                                        source: Some("mutica-lsp".to_string()),
                                        message,
                                        related_information: None,
                                        tags: None,
                                        data: None,
                                    });
                                }
                            }
                            ParseError::InternalError(msg) => {
                                let start = Position {
                                    line: 0,
                                    character: 0,
                                };
                                let end = offset_to_position(content, content.len());
                                diagnostics.push(Diagnostic {
                                    range: Range { start, end },
                                    severity: Some(DiagnosticSeverity::ERROR),
                                    code: None,
                                    code_description: None,
                                    source: Some("mutica-lsp".to_string()),
                                    message: msg.clone(),
                                    related_information: None,
                                    tags: None,
                                    data: None,
                                });
                            }
                        }

                        if diagnostics.is_empty() {
                            // 兜底：发送整体诊断
                            let start = Position {
                                line: 0,
                                character: 0,
                            };
                            let end = offset_to_position(content, content.len());
                            diagnostics.push(Diagnostic {
                                range: Range { start, end },
                                severity: Some(DiagnosticSeverity::ERROR),
                                code: None,
                                code_description: None,
                                source: Some("mutica-lsp".to_string()),
                                message: plain.clone(),
                                related_information: None,
                                tags: None,
                                data: None,
                            });
                        }

                        self.client
                            .publish_diagnostics(uri.clone(), diagnostics, None)
                            .await;
                    }
                }

                let mapping = SourceMapping::from_ast(&linearized, &source_file);

                // 生成tokens - 按行处理,避免跨行token
                let mut tokens = Vec::new();
                let mut last_line = 0u32;
                let mut last_start = 0u32;

                let lines: Vec<&str> = content.split('\n').collect();
                let mut byte_offset = 0;

                for (line_num, line_content) in lines.iter().enumerate() {
                    let line_start = byte_offset;
                    let line_end = byte_offset + line_content.len();

                    let mut current_start: Option<usize> = None;
                    let mut current_type: Option<u32> = None;

                    // 处理当前行的每个字节
                    for i in line_start..line_end {
                        let ty = mapping
                            .mapping()
                            .get(i)
                            .and_then(|node_opt| node_opt.as_ref())
                            .map(|node| self.ast_node_to_token_type(&node.value()))
                            .unwrap_or(17); // COMMENT

                        if current_type != Some(ty) {
                            // 输出之前的token
                            if let (Some(start), Some(typ)) = (current_start, current_type) {
                                let length = i - start;
                                let col = start - line_start;
                                let delta_line = line_num as u32 - last_line;
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
                                last_line = line_num as u32;
                                last_start = col as u32;
                            }
                            current_start = Some(i);
                            current_type = Some(ty);
                        }
                    }

                    // 输出本行最后一个token
                    if let (Some(start), Some(typ)) = (current_start, current_type) {
                        let length = line_end - start;
                        let col = start - line_start;
                        let delta_line = line_num as u32 - last_line;
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
                        last_line = line_num as u32;
                        last_start = col as u32;
                    }

                    // 移动到下一行(+1 for \n)
                    byte_offset = line_end + 1;
                }
                Ok(Some(SemanticTokens {
                    result_id: None,
                    data: tokens,
                }))
            }
            Err(e) => {
                // 将解析错误格式化并发送为 LSP Diagnostic
                let err_report = SyntaxError::new(e).report(uri.to_string(), content);
                let cache = (
                    uri.to_string(),
                    mutica::mutica_compiler::ariadne::Source::from(content),
                );
                let plain = report_to_plain_text(|buf: &mut Vec<u8>| err_report.write(cache, buf));
                let _ = std::io::stderr().write_all(plain.as_bytes());

                // 尝试从 SyntaxError / ErrorRecovery 中获取 span，发布诊断
                // 如果无法得到更精确的范围，则将诊断范围设为整个文档起始位置
                let mut diagnostics = Vec::new();
                // SyntaxError::new(e).report(...) 返回 Report, 但我们仍然可以
                // 使用 lalrpop 的 calculate_full_error_span 逻辑 if we had ErrorRecovery.
                // 退路：将整个文档的起始位置作为错误范围，确保客户端能显示诊断。
                let start = Position {
                    line: 0,
                    character: 0,
                };
                let end = offset_to_position(content, content.len());
                diagnostics.push(Diagnostic {
                    range: Range { start, end },
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: None,
                    code_description: None,
                    source: Some("mutica-lsp".to_string()),
                    message: plain,
                    related_information: None,
                    tags: None,
                    data: None,
                });

                self.client
                    .publish_diagnostics(uri.clone(), diagnostics, None)
                    .await;

                // 解析失败，返回 None（保持客户端已有着色）
                Ok(None)
            }
        }
    }

    fn ast_node_to_token_type(&self, node: &LinearTypeAst) -> u32 {
        match node {
            // Map AST nodes to indices in the expanded legend above.
            LinearTypeAst::Variable(_) => 8, // VARIABLE (index 8)
            LinearTypeAst::Pattern { .. } => 10, // ENUM_MEMBER (10)
            LinearTypeAst::Closure { .. } => 12, // FUNCTION (12)
            LinearTypeAst::Invoke { .. } => 11, // EVENT (11)
            LinearTypeAst::FixPoint { .. } => 12, // FUNCTION
            LinearTypeAst::Int => 1,         // TYPE (1)
            LinearTypeAst::Char => 1,        // TYPE
            LinearTypeAst::Top => 1,         // TYPE
            LinearTypeAst::Bottom => 1,      // TYPE
            LinearTypeAst::Tuple(_) => 5,    // STRUCT (5)
            LinearTypeAst::List(_) => 5,     // STRUCT
            LinearTypeAst::Generalize(_) => 6, // TYPE_PARAMETER (6)
            LinearTypeAst::Specialize(_) => 6, // TYPE_PARAMETER
            LinearTypeAst::Namespace { .. } => 0, // NAMESPACE (0)
            LinearTypeAst::IntLiteral(_) => 19, // NUMBER (19)
            LinearTypeAst::CharLiteral(_) => 18, // STRING (18)
            LinearTypeAst::Literal(_) => 18, // STRING
            LinearTypeAst::AtomicOpcode(_) => 15, // KEYWORD (15)
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
        last_tokens: RwLock::new(HashMap::new()),
        reference_table: RwLock::new(HashMap::new()),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
