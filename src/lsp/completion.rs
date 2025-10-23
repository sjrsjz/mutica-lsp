use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Position, Url};

pub fn get_completion_items() -> Vec<CompletionItem> {
    let keywords = vec![
        "let",
        "with",
        "match",
        "rec",
        "loop",
        "panic",
        "discard",
        "int",
        "char",
        "true",
        "false",
        "any",
        "none",
        "import",
        "if",
        "then",
        "else",
        "rot",
        "handle",
        "__add",
        "__sub",
        "__mul",
        "__div",
        "__mod",
        "__is",
        "__greater",
        "__less",
        "__opcode",
    ];

    let operators = vec![
        "->", "|->", "=>", "::", ".", "@", "|", "!", ":", "~", ",", "&", "==", "!=", "<", "<=",
        ">", ">=", "<:", "+", "-", "*", "/", "%", "=", ";", "#", "\\", "(", ")", "[", "]", "{",
        "}", "|>", "..",
    ];

    let functions = vec![
        "input!", "print!", "println!", "flush!", "repr!", "display!", "perform!", "break!",
        "resume!", "alloc!", "dealloc!", "set!", "get!",
    ];

    let mut items = Vec::new();

    for kw in keywords {
        items.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        });
    }

    for op in operators {
        items.push(CompletionItem {
            label: op.to_string(),
            kind: Some(CompletionItemKind::OPERATOR),
            ..Default::default()
        });
    }

    for func in functions {
        items.push(CompletionItem {
            label: func.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            ..Default::default()
        });
    }

    items
}

/// 根据变量映射提取变量补全项
pub fn get_variable_completions(
    uri: &Url,
    position: Position,
    documents: &RwLock<HashMap<Url, String>>,
    variable_maps: &RwLock<HashMap<Url, Vec<Option<Vec<String>>>>>,
) -> Option<Vec<CompletionItem>> {
    // 获取文档内容
    let content = documents.read().ok()?.get(uri).cloned()?;

    // 计算字节偏移
    let mut byte_offset = position_to_offset(&content, position)?;

    // 获取变量映射
    let maps = variable_maps.read().ok()?;
    let variable_vec = maps.get(uri)?;

    // 边界检查：如果偏移超出范围（文件末尾），使用最后一个有效位置
    if byte_offset >= variable_vec.len() {
        byte_offset = variable_vec.len().saturating_sub(1);
    }

    // 从当前位置开始向前查找最近的有变量信息的位置
    let variables = (0..=byte_offset)
        .rev()
        .find_map(|offset| variable_vec.get(offset)?.as_ref())?;

    // 去重变量名并生成补全项
    let mut unique_vars: HashSet<String> = HashSet::new();
    let mut items = Vec::new();

    for var_name in variables {
        if unique_vars.insert(var_name.clone()) {
            items.push(CompletionItem {
                label: var_name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some("Variable from context".to_string()),
                ..Default::default()
            });
        }
    }

    Some(items)
}

/// 将 Position 转换为字节偏移
fn position_to_offset(content: &str, position: Position) -> Option<usize> {
    let mut byte_offset = 0;
    let mut current_line = 0;

    for line in content.split('\n') {
        if current_line == position.line as usize {
            // 找到目标行，计算列偏移
            let mut char_count = 0;
            let mut col_offset = 0;

            for (i, _) in line.char_indices() {
                if char_count >= position.character as usize {
                    break;
                }
                char_count += 1;
                col_offset = i;
            }

            // 如果字符数正好等于，需要指向下一个位置
            if char_count == position.character as usize && char_count < line.chars().count() {
                col_offset = line
                    .char_indices()
                    .nth(char_count)
                    .map(|(i, _)| i)
                    .unwrap_or(line.len());
            } else if char_count < position.character as usize {
                col_offset = line.len();
            }

            return Some(byte_offset + col_offset);
        }

        byte_offset += line.len() + 1; // +1 for '\n'
        current_line += 1;
    }

    None
}
