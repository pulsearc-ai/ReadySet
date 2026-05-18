//! Manage the `.gitignore` ready-set block.

use crate::templates::{GITIGNORE, GITIGNORE_BEGIN, GITIGNORE_END};

/// Plan the gitignore content for a project root.
///
/// Given the current file content (or `None` if the file does not yet exist),
/// returns `Some(new_content)` when a write is required, or `None` when the
/// file already matches the desired state.
#[must_use]
pub fn plan(current: Option<&str>) -> Option<String> {
    let desired = render();
    match current {
        None => Some(desired),
        Some(text) if text == desired => None,
        Some(text) => Some(merge_managed(text, &desired)),
    }
}

fn render() -> String {
    let mut out = String::new();
    out.push_str(GITIGNORE_BEGIN);
    out.push('\n');
    out.push_str(GITIGNORE.trim_end());
    out.push('\n');
    out.push_str(GITIGNORE_END);
    out.push('\n');
    out
}

fn merge_managed(existing: &str, managed_block: &str) -> String {
    let begin = format!("{GITIGNORE_BEGIN}\n");
    let end = format!("{GITIGNORE_END}\n");

    if let (Some(b), Some(e)) = (existing.find(&begin), existing.find(&end))
        && b < e
    {
        let mut out = String::with_capacity(existing.len() + managed_block.len());
        out.push_str(&existing[..b]);
        out.push_str(managed_block);
        let after = e + end.len();
        if after < existing.len() {
            out.push_str(&existing[after..]);
        }
        return out;
    }

    let mut out = String::with_capacity(existing.len() + managed_block.len() + 1);
    out.push_str(existing);
    if !existing.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(managed_block);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_content_is_a_noop() {
        let target = render();
        assert!(plan(Some(&target)).is_none());
    }
}
