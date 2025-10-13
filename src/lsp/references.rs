use crate::lsp::utils::offset_to_position;
use mutica::mutica_compiler::parser::{
    SourceFile, WithLocation,
    ast::{FlowedMetaData, LinearTypeAst},
};
use tower_lsp::lsp_types::Range;

/// 递归遍历 AST 节点收集引用信息
pub fn collect_references<'ast>(
    node: &WithLocation<LinearTypeAst<'ast>, FlowedMetaData<'ast>>,
    table: &mut Vec<(Range, Range)>,
    source_file: &SourceFile,
) {
    // 检查当前节点的 reference 字段
    if let Some(use_loc) = node.location() {
        if let Some(ref_with_loc) = node.payload().reference() {
            if let Some(def_loc) = ref_with_loc.location()
                && def_loc.source() == source_file
            {
                let use_span = use_loc.span();
                let def_span = def_loc.span();
                let content = source_file.content();
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
                    end: offset_to_position(content, def_span.start + name_len),
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
                collect_references(item, table, source_file);
            }
        }
        LinearTypeAst::Closure {
            pattern,
            body,
            fail_branch,
            ..
        } => {
            collect_references(pattern, table, source_file);
            collect_references(body, table, source_file);
            if let Some(fail) = fail_branch {
                collect_references(fail, table, source_file);
            }
        }
        LinearTypeAst::Invoke {
            func,
            arg,
            continuation,
        } => {
            collect_references(func, table, source_file);
            collect_references(arg, table, source_file);
            collect_references(continuation, table, source_file);
        }
        LinearTypeAst::Pattern { expr, .. } => {
            collect_references(expr, table, source_file);
        }
        LinearTypeAst::Namespace { expr, .. } => {
            collect_references(expr, table, source_file);
        }
        LinearTypeAst::FixPoint { expr, .. } => {
            collect_references(expr, table, source_file);
        }
        LinearTypeAst::Literal(inner) => {
            collect_references(inner, table, source_file);
        }
        // 叶子节点：Variable, Int, Char, Top, Bottom, IntLiteral, CharLiteral, AtomicOpcode
        _ => {}
    }
}
