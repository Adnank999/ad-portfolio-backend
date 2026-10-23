use crate::common::{
    error::{AppError, AppResult},
    validation::{escape, url},
};
use serde_json::Value;

pub fn render_document(document: &Value) -> AppResult<String> {
    if document.get("type").and_then(Value::as_str) != Some("doc")
        || document.to_string().len() > 1_000_000
    {
        return Err(AppError::Invalid(
            "editorContent must be a Tiptap doc no larger than 1 MB".into(),
        ));
    }
    let mut remaining = 5000_usize;
    let html = render_node(document, 0, &mut remaining)?;
    Ok(ammonia::Builder::default()
        .link_rel(Some("noopener noreferrer"))
        .clean(&html)
        .to_string())
}

fn render_node(node: &Value, depth: usize, remaining: &mut usize) -> AppResult<String> {
    if depth > 20 || *remaining == 0 {
        return Err(AppError::Invalid(
            "Editor document is too deeply nested or has too many nodes".into(),
        ));
    }
    *remaining -= 1;
    let kind = node
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Invalid("Every editor node requires a type".into()))?;
    let children = match node.get("content") {
        Some(Value::Array(nodes)) => nodes
            .iter()
            .map(|child| render_node(child, depth + 1, remaining))
            .collect::<AppResult<Vec<_>>>()?
            .join(""),
        None => String::new(),
        _ => {
            return Err(AppError::Invalid(
                "Editor node content must be an array".into(),
            ))
        }
    };
    let tag = match kind {
        "doc" => {
            if depth != 0 {
                return Err(AppError::Invalid(
                    "Nested documents are not supported".into(),
                ));
            }
            return Ok(children);
        }
        "text" => return render_text(node),
        "paragraph" => "p",
        "bulletList" => "ul",
        "orderedList" => "ol",
        "listItem" => "li",
        "blockquote" => "blockquote",
        "codeBlock" => "pre",
        "table" => "table",
        "tableRow" => "tr",
        "tableCell" => "td",
        "tableHeader" => "th",
        "hardBreak" => return Ok("<br>".into()),
        "horizontalRule" => return Ok("<hr>".into()),
        "heading" => {
            let level = node
                .pointer("/attrs/level")
                .and_then(Value::as_u64)
                .unwrap_or(2);
            if !(2..=6).contains(&level) {
                return Err(AppError::Invalid(
                    "Article headings must use levels 2 through 6".into(),
                ));
            }
            return Ok(format!("<h{level}>{children}</h{level}>"));
        }
        "image" => {
            let source = node
                .pointer("/attrs/src")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let alt = node
                .pointer("/attrs/alt")
                .and_then(Value::as_str)
                .unwrap_or_default();
            url(source, "image src", true, false)?;
            if alt.chars().count() > 500 {
                return Err(AppError::Invalid("Image alt text is too long".into()));
            }
            return Ok(format!(
                "<img src=\"{}\" alt=\"{}\">",
                escape(source),
                escape(alt)
            ));
        }
        _ => {
            return Err(AppError::Invalid(format!(
                "Unsupported editor node: {kind}"
            )))
        }
    };
    Ok(format!("<{tag}>{children}</{tag}>"))
}

fn render_text(node: &Value) -> AppResult<String> {
    let text = node.get("text").and_then(Value::as_str).unwrap_or_default();
    if text.len() > 200_000 {
        return Err(AppError::Invalid("Editor text node is too long".into()));
    }
    let mut rendered = escape(text);
    if let Some(marks) = node.get("marks") {
        let marks = marks
            .as_array()
            .ok_or_else(|| AppError::Invalid("Text marks must be an array".into()))?;
        if marks.len() > 10 {
            return Err(AppError::Invalid("Too many text marks".into()));
        }
        for mark in marks {
            let tag = match mark.get("type").and_then(Value::as_str).unwrap_or_default() {
                "bold" => "strong",
                "italic" => "em",
                "strike" => "s",
                "underline" => "u",
                "code" => "code",
                "link" => {
                    let href = mark
                        .pointer("/attrs/href")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    url(href, "link href", true, false)?;
                    rendered = format!("<a href=\"{}\">{rendered}</a>", escape(href));
                    continue;
                }
                _ => return Err(AppError::Invalid("Unsupported text mark".into())),
            };
            rendered = format!("<{tag}>{rendered}</{tag}>");
        }
    }
    Ok(rendered)
}

pub fn plain_text(html: &str) -> AppResult<String> {
    let tags = regex::Regex::new("<[^>]*>").map_err(|_| AppError::Internal)?;
    Ok(tags
        .replace_all(html, " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" "))
}
