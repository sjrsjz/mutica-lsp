use mutica::mutica_compiler::parser::{
    MultiFileBuilder, MultiFileBuilderError, ParseContext, ParseError, SourceFile, SyntaxError,
    ast::LinearizeContext, calculate_full_error_span, colorize::TokenColor, report_error_recovery,
};
use mutica::mutica_core::util::cycle_detector::FastCycleDetector;
use mutica::mutica_semantic::semantic::SourceMapping;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::Client;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

use crate::lsp::ast_processor::perr_to_message;
use crate::lsp::references::collect_references;
use crate::lsp::utils::{offset_to_position, report_to_plain_text};

/// 解析文档并生成语义tokens,同时收集引用表和变量上下文映射
pub async fn parse_and_generate_tokens(
    content: &str,
    uri: &Url,
    client: &Client,
) -> Result<(
    Option<SemanticTokens>,
    Vec<(Range, Range)>,
    Option<Vec<Option<Vec<String>>>>,
)> {
    let file_path = if let Ok(path) = uri.to_file_path() {
        path
    } else {
        PathBuf::from(uri.path())
    };

    // 1. 使用 MultiFileBuilder 构建 BasicTypeAst
    let mut imported_ast = HashMap::new();
    let mut path_detector = FastCycleDetector::new();
    let mut builder_errors = Vec::new();

    let mut builder =
        MultiFileBuilder::new(&mut imported_ast, &mut path_detector, &mut builder_errors);

    let (basic_ast_option, source) = builder.build(file_path.clone(), content.to_string());

    // 2. 统一处理所有构建过程中的错误
    let mut diagnostics = Vec::new();
    for builder_error in &builder_errors {
        if let Some(loc) = builder_error.location() {
            let error_file_path = loc.source().filepath();
            let error_content = loc.source().content();

            let (range, message) = match builder_error.value() {
                MultiFileBuilderError::SyntaxError(e) => {
                    let report =
                        SyntaxError::new(e.clone()).report(error_file_path.clone(), error_content);
                    let cache = (
                        error_file_path,
                        mutica::mutica_compiler::ariadne::Source::from(error_content),
                    );
                    let msg = report_to_plain_text(|buf: &mut Vec<u8>| report.write(cache, buf));

                    // For a full syntax error, span the whole file
                    let start = Position::new(0, 0);
                    let end = offset_to_position(error_content, error_content.len());
                    (Range { start, end }, msg)
                }
                MultiFileBuilderError::RecoveryError(e) => {
                    let (start_byte, end_byte) = calculate_full_error_span(e);
                    let start = offset_to_position(error_content, start_byte);
                    let end = offset_to_position(error_content, end_byte);

                    let report = report_error_recovery(e, &error_file_path, error_content);
                    let cache = (
                        error_file_path.as_str(),
                        mutica::mutica_compiler::ariadne::Source::from(error_content),
                    );
                    let msg = report_to_plain_text(|buf: &mut Vec<u8>| report.write(cache, buf));
                    (Range { start, end }, msg)
                }
                MultiFileBuilderError::IOError(e) => {
                    let span = loc.span();
                    let range = Range {
                        start: offset_to_position(error_content, span.start),
                        end: offset_to_position(error_content, span.end),
                    };
                    (range, format!("I/O Error: {}", e))
                }
            };

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("mutica-lsp".to_string()),
                message,
                ..Default::default()
            });
        }
    }

    // 3. 如果构建成功，则继续处理
    if let Some(basic_ast) = basic_ast_option {
        // 4. 语义分析和后续处理
        let linearized = basic_ast
            .0
            .linearize(&mut LinearizeContext::new(), basic_ast.0.location())
            .finalize();

        let mut semantic_errors = Vec::new();
        let flowed_result = linearized.flow(
            &mut ParseContext::new(),
            false,
            linearized.location(),
            &mut semantic_errors,
        );

        for e in semantic_errors {
            let file_path = e
                .location()
                .map(|loc| loc.source().filepath())
                .unwrap_or_else(|| file_path.to_string_lossy().to_string());
            let content = e
                .location()
                .map(|loc| loc.source().content())
                .unwrap_or(content);
            let err_report = e.report();
            // Note: semantic errors are reported against the main file's content
            let cache = (
                file_path,
                mutica::mutica_compiler::ariadne::Source::from(content),
            );
            let plain = report_to_plain_text(|buf: &mut Vec<u8>| err_report.write(cache, buf));

            let mut error_items = Vec::new();
            match e.value() {
                ParseError::UseBeforeDeclaration(ast, name) => {
                    if ast.location().is_none()
                        || ast.location().unwrap().source() != source.as_ref()
                    {
                        continue;
                    }
                    let item = ast
                        .location()
                        .map(|loc| {
                            let span = loc.span();
                            let start = offset_to_position(content, span.start);
                            let end = offset_to_position(content, span.end);
                            (
                                Range { start, end },
                                format!("Use of undeclared variable '{}'", name),
                                DiagnosticSeverity::ERROR,
                            )
                        })
                        .unwrap_or((
                            Range {
                                start: Position::new(0, 0),
                                end: offset_to_position(content, content.len()),
                            },
                            format!("Use of undeclared variable '{}'", name),
                            DiagnosticSeverity::ERROR,
                        ));
                    error_items.push(item);
                }
                ParseError::RedeclaredCaptureValue(ast, name) => {
                    if ast.location().is_none()
                        || ast.location().unwrap().source() != source.as_ref()
                    {
                        continue;
                    }
                    let item = name
                        .location()
                        .or_else(|| ast.location())
                        .map(|loc| {
                            let span = loc.span();
                            let start = offset_to_position(content, span.start);
                            let end = offset_to_position(content, span.end);
                            (
                                Range { start, end },
                                format!("Redeclared capture variable '{}'", name.value()),
                                DiagnosticSeverity::ERROR,
                            )
                        })
                        .unwrap_or((
                            Range {
                                start: Position::new(0, 0),
                                end: offset_to_position(content, content.len()),
                            },
                            "Redeclared capture variable".to_string(),
                            DiagnosticSeverity::ERROR,
                        ));
                    error_items.push(item);
                }
                ParseError::UnusedVariable(_, names) => {
                    for name_loc in names {
                        if name_loc.location().is_none()
                            || name_loc.location().unwrap().source() != source.as_ref()
                        {
                            continue;
                        }

                        let item = name_loc
                            .location()
                            .map(|loc| {
                                let span = loc.span();
                                let start = offset_to_position(content, span.start);
                                let end = offset_to_position(content, span.end);
                                (
                                    Range { start, end },
                                    format!(
                                        "Variable '{}' is declared but never used",
                                        name_loc.value()
                                    ),
                                    DiagnosticSeverity::WARNING,
                                )
                            })
                            .unwrap_or((
                                Range {
                                    start: Position::new(0, 0),
                                    end: offset_to_position(content, content.len()),
                                },
                                "Variable is declared but never used".to_string(),
                                DiagnosticSeverity::WARNING,
                            ));
                        error_items.push(item);
                    }
                }
                ParseError::AmbiguousPattern(ast)
                | ParseError::PatternOutOfParameterDefinition(ast)
                | ParseError::MissingBranch(ast) => {
                    if ast.location().is_none()
                        || ast.location().unwrap().source() != source.as_ref()
                    {
                        continue;
                    }
                    let item = ast
                        .location()
                        .map(|loc| {
                            let span = loc.span();
                            let start = offset_to_position(content, span.start);
                            let end = offset_to_position(content, span.end);
                            let msg = perr_to_message(&e).unwrap_or_else(|| plain.clone());
                            (Range { start, end }, msg, DiagnosticSeverity::ERROR)
                        })
                        .unwrap_or((
                            Range {
                                start: Position::new(0, 0),
                                end: offset_to_position(content, content.len()),
                            },
                            plain.clone(),
                            DiagnosticSeverity::ERROR,
                        ));
                    error_items.push(item);
                }
                ParseError::InternalError(msg) => {
                    let start = Position::new(0, 0);
                    let end = offset_to_position(content, content.len());
                    error_items.push((
                        Range { start, end },
                        msg.clone(),
                        DiagnosticSeverity::ERROR,
                    ));
                }
            }

            for (range, message, severity) in error_items {
                diagnostics.push(Diagnostic {
                    range,
                    severity: Some(severity),
                    source: Some("mutica-lsp".to_string()),
                    message,
                    ..Default::default()
                });
            }
        }

        client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;

        // 5. 生成语义 Token 和引用
        let mut reference_table = Vec::new();
        collect_references(flowed_result.ty(), &mut reference_table, source.as_ref());

        let source_file = Arc::new(SourceFile::new(Some(file_path), content.to_string()));
        let mapping = SourceMapping::from_ast(flowed_result.ty(), &source_file);

        // 提取变量上下文映射：用 Vec 按字节偏移存储变量列表
        let content_len = content.len();
        let mut variable_vec: Vec<Option<Vec<String>>> = vec![None; content_len];

        for (offset, node_opt) in mapping.mapping().iter().enumerate() {
            if let Some(node) = node_opt {
                let variables: Vec<String> = node
                    .payload()
                    .variable_context()
                    .iter()
                    .map(|v| v.value().clone())
                    .collect();
                if !variables.is_empty() {
                    variable_vec[offset] = Some(variables);
                }
            }
        }

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
                let ty = basic_ast
                    .1
                    .color_mapping()
                    .get(i)
                    .map(|node| color_to_token_type(node))
                    .unwrap_or(17); // Default token type

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
            Some(variable_vec),
        ))
    } else {
        // 如果构建失败,发送诊断信息并提前返回
        client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
        Ok((None, Vec::new(), None))
    }
}

fn color_to_token_type(color: &TokenColor) -> u32 {
    match color {
        TokenColor::UnSpecified => 17, // Default token type as Comment
        TokenColor::Keyword => 15,     // KEYWORD
        TokenColor::Identifier => 8,   // VARIABLE
        TokenColor::Declaration => 8,  // VARIABLE (with declaration modifier)
        TokenColor::Namespace => 0,    // NAMESPACE
        TokenColor::Literal => 10,     // ENUM_MEMBER
        TokenColor::Operator => 21,    // OPERATOR
        TokenColor::Comment => 17,     // COMMENT
        TokenColor::Whitespace => 17,  // COMMENT (as default/fallback)
        TokenColor::Punctuation => 21, // OPERATOR (punctuation treated as operator)
        TokenColor::Function => 12,    // FUNCTION
        TokenColor::Type => 1,         // TYPE
        TokenColor::Attribute => 9,    // PROPERTY
        TokenColor::Macro => 14,       // MACRO
        TokenColor::Number => 19,      // NUMBER
        TokenColor::String => 18,      // STRING
        TokenColor::Boolean => 15,     // KEYWORD (boolean as keyword)
        TokenColor::Error => 17,       // COMMENT (error as fallback)
    }
}
