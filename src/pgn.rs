use std::collections::HashMap;

/// A parsed PGN document: its tag pairs and the list of SAN moves (main line).
pub struct PgnGame {
    pub tags: HashMap<String, String>,
    pub moves: Vec<String>,
}

/// Parse a PGN string into its tags and move list.
///
/// Comments (`{...}`), recursive variations (`(...)`), numeric annotations
/// (`$n`) and move-number prefixes are stripped so that only the main-line SAN
/// moves remain. The seven standard tag pairs plus any others are captured.
pub fn parse_pgn(pgn: &str) -> Result<PgnGame, String> {
    let mut tags: HashMap<String, String> = HashMap::new();
    let mut rest = pgn;

    // Extract the tag section first.
    while let Some(start) = rest.find('[') {
        let end_rel = rest[start..]
            .find(']')
            .ok_or_else(|| "unterminated tag bracket".to_string())?;
        let end = start + end_rel;
        let inner = &rest[start + 1..end];
        let sp = inner
            .find(char::is_whitespace)
            .ok_or_else(|| format!("malformed tag: [{inner}]"))?;
        let key = inner[..sp].trim().to_string();
        let valpart = &inner[sp..];
        let vstart = valpart
            .find('"')
            .ok_or_else(|| format!("malformed tag value: [{inner}]"))?;
        let vrem = &valpart[vstart + 1..];
        let vend = vrem
            .find('"')
            .ok_or_else(|| format!("malformed tag value: [{inner}]"))?;
        let value = vrem[..vend].to_string();
        tags.insert(key, value);
        rest = &rest[end + 1..];
    }

    // Parse the movetext.
    let cleaned = strip_noise(rest);
    let mut moves = Vec::new();
    for tok in cleaned.split_whitespace() {
        let t = tok.trim();
        if t.is_empty() || is_move_number(t) || is_result(t) {
            continue;
        }
        moves.push(t.to_string());
    }

    if moves.is_empty() {
        return Err("no moves found in PGN".to_string());
    }

    Ok(PgnGame { tags, moves })
}

/// Render a PGN string from tags, the SAN move list and the result token.
pub fn render_pgn(
    tags: &HashMap<String, String>,
    sans: &[String],
    result: &str,
) -> String {
    let mut out = String::new();
    let standard = [
        "Event", "Site", "Date", "Round", "White", "Black", "Result",
    ];

    for key in standard {
        if let Some(v) = tags.get(key) {
            out.push_str(&format!("[{} \"{}\"]\n", key, escape_tag(v)));
        }
    }
    for (k, v) in tags {
        if !standard.contains(&k.as_str()) {
            out.push_str(&format!("[{} \"{}\"]\n", k, escape_tag(v)));
        }
    }
    out.push('\n');

    // Build the movetext tokens, numbering full moves.
    let mut tokens: Vec<String> = Vec::new();
    let mut move_num: u32 = 1;
    for (i, san) in sans.iter().enumerate() {
        if i % 2 == 0 {
            tokens.push(format!("{}.", move_num));
        }
        tokens.push(san.clone());
        if i % 2 == 1 {
            move_num += 1;
        }
    }
    tokens.push(result.to_string());

    // Join with single spaces, soft-wrapping at ~80 columns.
    let mut col = 0usize;
    for (i, tok) in tokens.iter().enumerate() {
        if i == 0 {
            out.push_str(tok);
            col = tok.len();
        } else if col + 1 + tok.len() > 80 {
            out.push('\n');
            out.push_str(tok);
            col = tok.len();
        } else {
            out.push(' ');
            out.push_str(tok);
            col += 1 + tok.len();
        }
    }
    out.push('\n');
    out
}

fn strip_noise(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' => {
                while i < chars.len() && chars[i] != '}' {
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
            }
            '(' => {
                let mut depth = 1;
                i += 1;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '(' {
                        depth += 1;
                    } else if chars[i] == ')' {
                        depth -= 1;
                    }
                    if depth == 0 {
                        break;
                    }
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
            }
            '$' => {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn is_move_number(t: &str) -> bool {
    let t = t.trim_end_matches('.');
    !t.is_empty() && t.chars().all(|c| c.is_ascii_digit())
}

fn is_result(t: &str) -> bool {
    matches!(t, "1-0" | "0-1" | "1/2-1/2" | "*")
}

fn escape_tag(v: &str) -> String {
    v.replace('"', "\\\"")
}
