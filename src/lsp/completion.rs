use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};

pub fn get_completion_items() -> Vec<CompletionItem> {
    let keywords = vec![
        "let",
        "perform",
        "with",
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
        "import",
        "__add",
        "__sub",
        "__mul",
        "__div",
        "__mod",
        "__is",
        "__opcode",
        "__continuation",
    ];

    let operators = vec![
        "->", "|->", "=>", "::", ".", "@", "|", "!", ":", "~", ",", "&", "==", "!=", "<", "<=",
        ">", ">=", "<:", "+", "-", "*", "/", "%", "=", ";", "#", "\\", "(", ")", "[", "]", "{",
        "}", "|>",
    ];

    let functions = vec!["input!", "print!", "println!", "flush!"];

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
