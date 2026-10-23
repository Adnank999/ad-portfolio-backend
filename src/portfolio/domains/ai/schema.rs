use super::dto::{ContentType, Field};
use serde_json::{json, Map, Value};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn rich_blocks() -> Value {
    let inline = object(
        json!({"text":{"type":"string"},"bold":{"type":"boolean"},"italic":{"type":"boolean"},"code":{"type":"boolean"}}),
        &["text", "bold", "italic", "code"],
    );
    let content = json!({"type":"array","items":inline});
    json!({"type":"array","items":{"anyOf":[
        object(json!({"type":{"type":"string","enum":["paragraph"]},"content":content}), &["type","content"]),
        object(json!({"type":{"type":"string","enum":["heading"]},"level":{"type":"integer","enum":[2,3]},"content":content}), &["type","level","content"]),
        object(json!({"type":{"type":"string","enum":["bulletList","orderedList"]},"items":{"type":"array","items":content}}), &["type","items"]),
        object(json!({"type":{"type":"string","enum":["blockquote"]},"content":content}), &["type","content"]),
        object(json!({"type":{"type":"string","enum":["codeBlock"]},"text":{"type":"string"},"language":{"type":["string","null"]}}), &["type","text","language"]),
        object(json!({"type":{"type":"string","enum":["table"]},"headers":{"type":"array","items":{"type":"string"}},"rows":{"type":"array","items":{"type":"array","items":{"type":"string"}}}}), &["type","headers","rows"])
    ]}})
}

pub fn response(kind: ContentType, fields: &[Field]) -> Value {
    let properties: Map<String, Value> = fields.iter().filter(|field| !field.derived()).map(|field| {
        let value = match field {
            Field::PageContent => crate::domains::content::services::page_schema().unwrap_or_else(|_| json!({"type":"object"})),
            Field::EditorContent => rich_blocks(),
            Field::Faqs => json!({"type":"array","items":object(json!({"question":{"type":"string"},"answer":{"type":"string"}}), &["question","answer"])}),
            Field::Tags | Field::Features | Field::Technologies | Field::Lessons => json!({"type":"array","items":{"type":"string"}}),
            Field::Category if kind == ContentType::Blog => json!({"type":"string","enum":["insights","growth","ai","news"]}),
            _ => json!({"type":"string"}),
        };
        (field.key().to_owned(), json!({"anyOf":[value,{"type":"null"}]}))
    }).collect();
    let required: Vec<&str> = fields
        .iter()
        .filter(|field| !field.derived())
        .map(|field| field.key())
        .collect();
    object(
        json!({"generatedFields":object(json!(properties), &required),"warnings":{"type":"array","items":{"type":"string"}}}),
        &["generatedFields", "warnings"],
    )
}
