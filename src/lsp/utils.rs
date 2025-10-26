use tower_lsp::lsp_types::{Position, Range};

/// 丢弃字符串中的 ANSI 控制序列
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if let Some('[') = chars.next() {
                // consume until a final byte in range '@'..='~'
                for n in chars.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Generic helper: write a report into an in-memory buffer via the provided closure,
/// strip ANSI codes and return the plain text string.
pub fn report_to_plain_text<F>(write_report: F) -> String
where
    F: FnOnce(&mut Vec<u8>) -> std::io::Result<()>,
{
    let mut buf: Vec<u8> = Vec::new();
    let _ = write_report(&mut buf);
    let out = String::from_utf8_lossy(&buf);
    strip_ansi(&out)
}

/// 将字节偏移转换为行列号
pub fn offset_to_position(content: &str, offset: usize) -> Position {
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

/// 辅助函数：判断位置是否在范围内
pub fn position_in_range(pos: &Position, range: &Range) -> bool {
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

/// 辅助函数：判断两个范围是否相等
pub fn ranges_equal(a: &Range, b: &Range) -> bool {
    a.start.line == b.start.line
        && a.start.character == b.start.character
        && a.end.line == b.end.line
        && a.end.character == b.end.character
}
