use super::dto::Field;
use crate::{
    common::{
        error::{AppError, AppResult},
        validation,
    },
    domains::content::editor,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inline {
    pub text: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub code: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Block {
    Paragraph {
        content: Vec<Inline>,
    },
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    BulletList {
        items: Vec<Vec<Inline>>,
    },
    OrderedList {
        items: Vec<Vec<Inline>>,
    },
    Blockquote {
        content: Vec<Inline>,
    },
    CodeBlock {
        text: String,
        language: Option<String>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

fn inlines(content: Vec<Inline>) -> AppResult<Vec<Value>> {
    if content.len() > 100 {
        return Err(AppError::AiInvalidOutput);
    }
    content
        .into_iter()
        .filter(|part| !part.text.is_empty())
        .map(|part| {
            validation::text(&part.text, "inline text", 1, 20_000)
                .map_err(|_| AppError::AiInvalidOutput)?;
            let mut marks = Vec::new();
            if part.bold {
                marks.push(json!({"type":"bold"}));
            }
            if part.italic {
                marks.push(json!({"type":"italic"}));
            }
            if part.code {
                marks.push(json!({"type":"code"}));
            }
            Ok(json!({"type":"text","text":part.text,"marks":marks}))
        })
        .collect()
}

pub fn document(value: Value) -> AppResult<Value> {
    let blocks: Vec<Block> =
        serde_json::from_value(value).map_err(|_| AppError::AiInvalidOutput)?;
    if blocks.is_empty() || blocks.len() > 160 {
        return Err(AppError::AiInvalidOutput);
    }
    let content = blocks.into_iter().map(|block| Ok(match block {
        Block::Paragraph { content } => json!({"type":"paragraph","content":inlines(content)?}),
        Block::Heading { level, content } => {
            if !matches!(level, 2 | 3) { return Err(AppError::AiInvalidOutput); }
            json!({"type":"heading","attrs":{"level":level},"content":inlines(content)?})
        }
        Block::BulletList { items } => list("bulletList", items)?,
        Block::OrderedList { items } => list("orderedList", items)?,
        Block::Blockquote { content } => json!({"type":"blockquote","content":[{"type":"paragraph","content":inlines(content)?}]}),
        Block::CodeBlock { text, language } => {
            validation::text(&text, "code", 1, 20_000).map_err(|_| AppError::AiInvalidOutput)?;
            if let Some(language) = &language { validation::text(language, "code language", 1, 40).map_err(|_| AppError::AiInvalidOutput)?; }
            json!({"type":"codeBlock","attrs":{"language":language},"content":[{"type":"text","text":text}]})
        }
        Block::Table { headers, rows } => {
            if headers.is_empty() || headers.len() > 8 || rows.len() > 30 || rows.iter().any(|row| row.len() != headers.len()) { return Err(AppError::AiInvalidOutput); }
            let mut output = vec![table_row(headers, "tableHeader")?];
            for row in rows { output.push(table_row(row, "tableCell")?); }
            json!({"type":"table","content":output})
        }
    })).collect::<AppResult<Vec<Value>>>()?;
    let document = json!({"type":"doc","content":content});
    if document.to_string().len() > 200_000 {
        return Err(AppError::AiInvalidOutput);
    }
    let html = editor::render_document(&document).map_err(|_| AppError::AiInvalidOutput)?;
    if editor::plain_text(&html)?.trim().is_empty() {
        return Err(AppError::AiInvalidOutput);
    }
    Ok(document)
}

fn list(kind: &str, items: Vec<Vec<Inline>>) -> AppResult<Value> {
    if items.is_empty() || items.len() > 50 {
        return Err(AppError::AiInvalidOutput);
    }
    let content = items
        .into_iter()
        .map(|item| {
            Ok(json!({"type":"listItem","content":[{"type":"paragraph","content":inlines(item)?}]}))
        })
        .collect::<AppResult<Vec<Value>>>()?;
    Ok(json!({"type":kind,"content":content}))
}

fn table_row(cells: Vec<String>, kind: &str) -> AppResult<Value> {
    let content = cells.into_iter().map(|text| {
        validation::text(&text, Field::EditorContent.key(), 1, 2000).map_err(|_| AppError::AiInvalidOutput)?;
        Ok(json!({"type":kind,"content":[{"type":"paragraph","content":[{"type":"text","text":text}]}]}))
    }).collect::<AppResult<Vec<Value>>>()?;
    Ok(json!({"type":"tableRow","content":content}))
}
