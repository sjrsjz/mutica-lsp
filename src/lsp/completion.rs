use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};

pub fn get_completion_items() -> Vec<CompletionItem> {
    let keywords = vec![
        "let",
        "perform",
        "do",
        "with",
        "as",
        "match",
        "rec",
        "panic",
        "discard",
        "int",
        "char",
        "true",
        "false",
        "any",
        "none",
        "__add",
        "__sub",
        "__mul",
        "__div",
        "__mod",
        "__is",
        "__opcode",
        "__continuation",
        "input",
        "print",
    ];

    let operators = vec![
        "->", "|->", "=>", "::", ".", "@", "|", "!", ":", "~", ",", "&", "==", "!=", "<", "<=",
        ">", ">=", "<:", "+", "-", "*", "/", "%", "=", ";", "#", "\\", "(", ")", "[", "]", "{",
        "}", "|>",
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

    items
}
