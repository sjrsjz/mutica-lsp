use crate::lsp::utils::offset_to_position;
use mutica::{
    mutica_compiler::parser::{
        WithLocation,
        ast::{FlowedMetaData, LinearTypeAst},
    },
    mutica_core::util::source_info::SourceFile,
};
use tower_lsp::lsp_types::{Location, Range, Url};

/// 递归遍历 AST 节点收集引用信息
/// 返回值为 (use_range, def_location)，支持跨文件引用
#[stacksafe::stacksafe]
pub fn collect_references(
    node: &WithLocation<LinearTypeAst, FlowedMetaData>,
    table: &mut Vec<(Range, Location)>,
    source_file: &SourceFile,
) {
    // 递归遍历所有子节点
    match node.value() {
        LinearTypeAst::AllOf(items) | LinearTypeAst::AnyOf(items) => {
            for item in items {
                collect_references(item, table, source_file);
            }
        }
        LinearTypeAst::Tuple(items) => {
            for item in items {
                collect_references(&item.0, table, source_file);
            }
        }
        LinearTypeAst::Cons { head, tail } => {
            for item in head {
                collect_references(&item.0, table, source_file);
            }
            collect_references(tail, table, source_file);
        }
        LinearTypeAst::List { head, tail } => {
            for item in head {
                collect_references(&item.0, table, source_file);
            }
            collect_references(tail, table, source_file);
        }
        LinearTypeAst::Match { branches, .. } => {
            for (p, c, expr) in branches {
                collect_references(expr, table, source_file);
                for (_, c) in c {
                    collect_references(c, table, source_file);
                }
                collect_references(p, table, source_file);
            }
        }
        LinearTypeAst::Generic {
            expr, constraint, ..
        } => {
            collect_references(expr, table, source_file);
            for (_, c) in constraint {
                collect_references(c, table, source_file);
            }
        }
        LinearTypeAst::Invoke {
            func,
            arg,
            continuation,
            perform_handler,
        } => {
            collect_references(func, table, source_file);
            collect_references(arg, table, source_file);
            if let Some(continuation) = continuation {
                collect_references(continuation, table, source_file);
            }
            if let Some(handler) = perform_handler {
                collect_references(handler, table, source_file);
            }
        }
        LinearTypeAst::Namespace { expr, .. } => {
            collect_references(expr, table, source_file);
        }
        LinearTypeAst::Bind { expr, .. } => {
            collect_references(expr, table, source_file);
        }
        LinearTypeAst::Lazy(inner) => {
            collect_references(inner, table, source_file);
        }
        LinearTypeAst::Range { ty, .. } => {
            collect_references(ty, table, source_file);
        }
        LinearTypeAst::Char => {}
        LinearTypeAst::Float => {}
        LinearTypeAst::NaturalNumberSet => {}
        LinearTypeAst::Lambda { patterns } => {
            for (p, c) in patterns {
                for (_, c) in c {
                    collect_references(c, table, source_file);
                }
                collect_references(p, table, source_file);
            }
        }
        LinearTypeAst::FloatLiteral(_) => {}
        LinearTypeAst::CharLiteral(_) => {}
        LinearTypeAst::NaturalNumberLiteral(_) => {}
        LinearTypeAst::Variable(_) => {
            // 检查当前节点的 reference 字段
            if let Some(use_loc) = node.location()
                && let Some(ref_with_loc) = node.payload().reference()
                && let Some(def_loc) = ref_with_loc.location()
            // && use_loc.source() == def_loc.source()
                && use_loc.source() == source_file
            {
                let use_span = use_loc.span();
                let def_span = def_loc.span();
                let use_content = source_file.content();
                let def_content = def_loc.source().content();

                // // DEBUG: 提取使用和定义处的实际文本
                let use_text = use_content
                    .get(use_span.clone())
                    .unwrap_or("<invalid span>");
                let def_text = def_content
                    .get(def_span.clone())
                    .unwrap_or("<invalid span>");

                eprintln!("=== REFERENCE FOUND ===");
                eprintln!("Usage:");
                eprintln!("  File: {}", use_loc.source().filepath());
                eprintln!(
                    "  Span: {}..{} (len: {})",
                    use_span.start,
                    use_span.end,
                    use_span.len()
                );
                eprintln!("  Text: {:?}", use_text);
                eprintln!("Definition:");
                eprintln!("  File: {}", def_loc.source().filepath());
                eprintln!(
                    "  Span: {}..{} (len: {})",
                    def_span.start,
                    def_span.end,
                    def_span.len()
                );
                eprintln!("  Text: {:?}", def_text);

                let use_range = Range {
                    start: offset_to_position(use_content, use_span.start),
                    end: offset_to_position(use_content, use_span.end),
                };

                let def_range = Range {
                    start: offset_to_position(def_content, def_span.start),
                    end: offset_to_position(def_content, def_span.end),
                };

                eprintln!("  Def Range: {:?}", def_range);

                // // 将定义文件路径转换为 URI
                let def_uri = if let Some(def_path) = def_loc.source().path() {
                    eprintln!("  Def Path: {:?}", def_path);
                    match Url::from_file_path(def_path) {
                        Ok(uri) => {
                            eprintln!("  Def URI: {}", uri);
                            uri
                        }
                        Err(_) => {
                            eprintln!("  ERROR: Failed to convert path to URI");
                            // 如果路径转换失败，跳过这个引用
                            return;
                        }
                    }
                } else {
                    eprintln!("  ERROR: No path in def_loc");
                    // 如果没有路径信息，跳过这个引用
                    return;
                };

                let def_location = Location {
                    uri: def_uri,
                    range: def_range,
                };

                eprintln!("======================\n");

                table.push((use_range, def_location));
            }
        }
        LinearTypeAst::AtomicOpcode(_) => {}
        LinearTypeAst::SubOf { value } => {
            collect_references(value, table, source_file);
        }
        LinearTypeAst::Mutable { value } => {
            collect_references(value, table, source_file);
        }
        LinearTypeAst::StaticFixPoint { expr, .. } => {
            collect_references(expr, table, source_file);
        }
    }
}
