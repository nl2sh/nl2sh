//! WeChat Markdown compatibility: retain supported syntax without changing code literals.
use regex::Regex;
use std::sync::OnceLock;

pub(super) fn compatible(source: &str) -> String {
    static IMAGE: OnceLock<Option<Regex>> = OnceLock::new();
    let image = IMAGE.get_or_init(|| Regex::new(r"!\[[^\]\n]*\]\([^\)\n]*\)").ok());
    let mut fence: Option<String> = None;
    let mut out = String::new();
    for line in source.split_inclusive('\n') {
        if update_fence(line, &mut fence) {
            out.push_str(line);
            continue;
        }
        if fence.is_some() {
            out.push_str(line);
            continue;
        }
        let line = line
            .strip_prefix("###### ")
            .or_else(|| line.strip_prefix("##### "))
            .unwrap_or(line);
        // Inline code is copied verbatim; filtering applies only to prose segments.
        for (index, segment) in line.split('`').enumerate() {
            if index > 0 {
                out.push('`');
            }
            if index % 2 == 1 {
                out.push_str(segment);
                continue;
            }
            let segment = image
                .as_ref()
                .map(|r| r.replace_all(segment, "").into_owned())
                .unwrap_or_else(|| segment.to_owned());
            out.push_str(&emphasis(&segment));
        }
    }
    out
}
fn emphasis(source: &str) -> String {
    let mut rest = source;
    let mut output = String::new();
    while !rest.is_empty() {
        let first = rest.chars().next().unwrap_or_default();
        if first == '*' || first == '_' {
            let count = rest.chars().take_while(|c| *c == first).count();
            let marker = &rest[..count];
            if let Some(end) = rest[count..].find(marker) {
                let text = &rest[count..count + end];
                let cjk = text
                    .chars()
                    .any(|c| matches!(c as u32, 0x2e80..=0x9fff|0xac00..=0xd7af|0xf900..=0xfaff));
                if matches!(count, 1 | 3) && cjk {
                    output.push_str(text);
                } else {
                    output.push_str(&rest[..count + end + count]);
                }
                rest = &rest[count + end + count..];
                continue;
            }
            output.push_str(marker);
            rest = &rest[count..];
            continue;
        }
        output.push(first);
        rest = &rest[first.len_utf8()..];
    }
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_unsupported_prose_and_preserves_code_tables_and_bold() {
        let source = "##### 标题\n*中文* **粗体** ***中文*** _中文_ ![图](https://x)\n|列|值|\n`*中文*`\n```sh\n##### code\n![literal](x)\n```\n";
        let out = compatible(source);
        assert!(out.starts_with("标题\n中文 **粗体** 中文 中文 "));
        assert!(out.contains("|列|值|"));
        assert!(out.contains("`*中文*`"));
        assert!(out.contains("##### code\n![literal](x)"));
    }
}

/// Split native Markdown at line boundaries and close/reopen code fences between messages.
pub(super) fn split(source: &str, max: usize) -> Vec<String> {
    let longest = source
        .lines()
        .filter_map(marker)
        .map(|m| m.len())
        .max()
        .unwrap_or(3);
    let budget = max.saturating_sub(longest * 2 + 8).max(longest + 2);
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut fence: Option<String> = None;
    for line in source.split_inclusive('\n') {
        for fragment in super::chunks(line, budget) {
            if current.len() + fragment.len() > budget && !current.is_empty() {
                if let Some(marker) = &fence {
                    current.push_str(&format!("\n{marker}\n"));
                }
                parts.push(std::mem::take(&mut current));
                if let Some(marker) = &fence {
                    current.push_str(&format!("{marker}\n"));
                }
            }
            current.push_str(&fragment);
        }
        update_fence(line, &mut fence);
    }
    if !current.is_empty() {
        if let Some(marker) = &fence {
            current.push_str(&format!("\n{marker}\n"));
        }
        parts.push(current);
    }
    parts
}
#[cfg(test)]
mod split_tests {
    use super::*;
    #[test]
    fn native_chunks_preserve_utf8_and_balance_long_code_blocks() {
        let code = "中文命令行\n".repeat(1200);
        let parts = split(&format!("# 结果\n```sh\n{code}```\n"), 3500);
        assert!(parts.len() > 1);
        for part in parts {
            assert!(part.len() <= 3500);
            assert_eq!(
                part.lines()
                    .filter(|l| l.trim_start().starts_with("```"))
                    .count()
                    % 2,
                0
            );
        }
    }
}

/// Render untrusted operation text verbatim, including any embedded Markdown fences.
pub(super) fn literal(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let tilde_longest = text.split(|c| c != '~').map(str::len).max().unwrap_or(0);
    let fence = if longest > 32 && tilde_longest < longest {
        "~".repeat((tilde_longest + 1).max(3))
    } else {
        "`".repeat((longest + 1).max(3))
    };
    format!("{fence}\n{text}\n{fence}")
}
fn marker(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let character = line.chars().next()?;
    if !matches!(character, '`' | '~') {
        return None;
    }
    let length = line.chars().take_while(|c| *c == character).count();
    (length >= 3).then(|| &line[..length])
}
fn update_fence(line: &str, fence: &mut Option<String>) -> bool {
    let Some(candidate) = marker(line) else {
        return false;
    };
    match fence {
        Some(open)
            if candidate.starts_with(open.as_str())
                && line.trim_start()[candidate.len()..].trim().is_empty() =>
        {
            *fence = None;
            true
        }
        Some(_) => false,
        None => {
            *fence = Some(candidate.into());
            true
        }
    }
}
#[cfg(test)]
mod literal_tests {
    use super::*;
    #[test]
    fn approval_preview_cannot_close_its_fence_or_hide_commands() {
        let preview = "rm -rf /tmp/test\n```\n![literal](url) *中文*\n````";
        let formatted = literal(preview);
        assert!(formatted.starts_with("`````\n"));
        assert!(formatted.contains(preview));
        assert_eq!(compatible(&formatted), formatted);
        let parts = split(&literal(&preview.repeat(100)), 3500);
        assert!(parts.len() > 1);
        for part in parts {
            assert!(part.len() <= 3500);
            assert!(part.starts_with("`````\n"));
            assert!(part.trim_end().ends_with("`````"));
        }
    }
}
