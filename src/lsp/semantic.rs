use mutica::mutica_compiler::SyntaxError;
use mutica::mutica_compiler::parser::{
    ParseContext, ParseError, calculate_full_error_span, report_error_recovery,
};
use mutica::mutica_compiler::parser::{SourceFile, ast::LinearTypeAst};
use mutica::mutica_compiler::{
    grammar::TypeParser,
    logos::Logos,
    parser::{ast::LinearizeContext, lexer::LexerToken},
};
use mutica::mutica_semantic::semantic::SourceMapping;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::Client;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

use crate::lsp::ast_processor::{perr_to_message, sanitize_ast};
use crate::lsp::references::collect_references;
use crate::lsp::utils::{offset_to_position, report_to_plain_text};

/// 解析文档并生成语义tokens，同时收集引用表
pub async fn parse_and_generate_tokens(
    content: &str,
    uri: &Url,
    client: &Client,
) -> Result<(Option<SemanticTokens>, Vec<(Range, Range)>)> {
    // 尝试使用 to_file_path() 获取文件系统路径
    let file_path = if let Ok(path) = uri.to_file_path() {
        path.to_string_lossy().to_string()
    } else {
        uri.path().to_string()
    };

    let source_file = Arc::new(SourceFile::new(
        Some(PathBuf::from(&file_path)),
        content.into(),
    ));
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
            let mut diagnostics = Vec::new();

            for err in &errors {
                let (start_byte, end_byte) = calculate_full_error_span(err);
                let start = offset_to_position(content, start_byte);
                let end = offset_to_position(content, end_byte);

                // 使用实际的文件路径
                let report = report_error_recovery(err, &file_path, content);
                let cache = (
                    file_path.as_str(),
                    mutica::mutica_compiler::ariadne::Source::from(content),
                );
                let message = report_to_plain_text(|buf: &mut Vec<u8>| report.write(cache, buf));

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

            client
                .publish_diagnostics(uri.clone(), diagnostics, None)
                .await;

            // 清理 AST 中的 ParseError 节点
            let sanitized_ast = sanitize_ast(ast);
            let basic = sanitized_ast.into_basic(sanitized_ast.location());
            let linearized = basic
                .linearize(&mut LinearizeContext::new(), basic.location())
                .finalize();

            let mut errors = Vec::new();
            let flowed_result = linearized.flow(
                &mut ParseContext::new(),
                false,
                linearized.location(),
                &mut errors,
            );
            let mut diagnostics: Vec<Diagnostic> = Vec::new();

            for e in errors {
                let err_report = e.report();
                let cache = (
                    file_path.clone(),
                    mutica::mutica_compiler::ariadne::Source::from(content),
                );
                let plain = report_to_plain_text(|buf: &mut Vec<u8>| err_report.write(cache, buf));
                let _ = std::io::stderr().write_all(plain.as_bytes());

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
                            let message = format!("Redeclared pattern variable '{}'", name.value());
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
                                    severity: Some(DiagnosticSeverity::WARNING),
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
            }

            client
                .publish_diagnostics(uri.clone(), diagnostics, None)
                .await;
            let mut reference_table = Vec::new();

            collect_references(flowed_result.ty(), content, &mut reference_table); // // 将引用表输出到 stderr，便于调试

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

                for i in line_start..line_end {
                    let ty = mapping
                        .mapping()
                        .get(i)
                        .and_then(|node_opt| node_opt.as_ref())
                        .map(|node| ast_node_to_token_type(&node.value()))
                        .unwrap_or(17);

                    if current_type != Some(ty) {
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

                byte_offset = line_end + 1;
            }

            Ok((
                Some(SemanticTokens {
                    result_id: None,
                    data: tokens,
                }),
                reference_table,
            ))
        }
        Err(e) => {
            // 将解析错误格式化并发送为 LSP Diagnostic
            let err_report = SyntaxError::new(e).report(file_path.clone(), content);
            let cache = (
                file_path.clone(),
                mutica::mutica_compiler::ariadne::Source::from(content),
            );
            let plain = report_to_plain_text(|buf: &mut Vec<u8>| err_report.write(cache, buf));
            let _ = std::io::stderr().write_all(plain.as_bytes());

            let mut diagnostics = Vec::new();
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

            client
                .publish_diagnostics(uri.clone(), diagnostics, None)
                .await;

            Ok((None, Vec::new()))
        }
    }
}

fn ast_node_to_token_type(node: &LinearTypeAst) -> u32 {
    match node {
        LinearTypeAst::Variable(_) => 8,
        LinearTypeAst::Pattern { .. } => 10,
        LinearTypeAst::Closure { .. } => 12,
        LinearTypeAst::Invoke { .. } => 11,
        LinearTypeAst::FixPoint { .. } => 12,
        LinearTypeAst::Int => 1,
        LinearTypeAst::Char => 1,
        LinearTypeAst::Top => 1,
        LinearTypeAst::Bottom => 1,
        LinearTypeAst::Tuple(_) => 5,
        LinearTypeAst::List(_) => 5,
        LinearTypeAst::Generalize(_) => 6,
        LinearTypeAst::Specialize(_) => 6,
        LinearTypeAst::Namespace { .. } => 0,
        LinearTypeAst::IntLiteral(_) => 19,
        LinearTypeAst::CharLiteral(_) => 18,
        LinearTypeAst::Literal(_) => 18,
        LinearTypeAst::AtomicOpcode(_) => 15,
    }
}
