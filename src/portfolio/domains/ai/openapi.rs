use super::dto::Field;
use serde_json::{json, Value};

pub fn extend(document: &mut Value) {
    let fields: Vec<&str> = Field::ALL.iter().map(|field| field.key()).collect();
    document["components"]["schemas"]["AiGenerationRequest"] = json!({
        "type":"object","additionalProperties":false,
        "required":["contentType","mode","context","tone","audience","length","language","targetFields","contentContext"],
        "properties":{
            "contentType":{"type":"string","enum":["blog","project"]},
            "mode":{"type":"string","enum":["create","regenerate","fill_missing"]},
            "context":{"type":"string","minLength":20,"maxLength":20000},
            "additionalInstructions":{"type":"string","maxLength":4000},
            "tone":{"type":"string","enum":["professional","technical","conversational","storytelling","educational","case_study"]},
            "audience":{"type":"string","enum":["general_readers","software_developers","senior_engineers","technical_recruiters","business_owners","potential_clients"]},
            "length":{"type":"string","enum":["short","medium","long","in_depth"]},
            "language":{"type":"string","maxLength":80},
            "seoKeywords":{"type":"array","maxItems":20,"items":{"type":"string","maxLength":100}},
            "targetFields":{"type":"array","minItems":1,"uniqueItems":true,"items":{"type":"string","enum":fields}},
            "existingFields":{"type":"object","description":"Relevant current CMS values keyed by allowlisted field paths. May also contain fields.cover or fields.image/liveUrl/githubUrl. Publishing controls are rejected."},
            "contentContext":{"oneOf":[
                {"type":"object","additionalProperties":false,"required":["type"],"properties":{
                    "type":{"const":"blog"},"topic":{"type":"string"},"argument":{"type":"string"},"concepts":{"type":"string"},"technologies":{"type":"array","items":{"type":"string"}},"examples":{"type":"string"},"references":{"type":"string"},"structure":{"type":"string"},"imageDescription":{"type":"string"}
                }},
                {"type":"object","additionalProperties":false,"required":["type"],"properties":{
                    "type":{"const":"project"},"name":{"type":"string"},"projectType":{"type":"string"},"technologies":{"type":"array","items":{"type":"string"}},"role":{"type":"string"},"problem":{"type":"string"},"solution":{"type":"string"},"features":{"type":"array","items":{"type":"string"}},"challenges":{"type":"string"},"outcomes":{"type":"string"},"architecture":{"type":"string"},"year":{"type":"string"},"imageDescription":{"type":"string"}
                }}
            ]}
        },
        "example":{"contentType":"blog","mode":"create","context":"Explain my Rust and PostgreSQL booking backend and its Redis caching layer.","tone":"technical","audience":"software_developers","length":"medium","language":"English","targetFields":["title","description","editorContent","seo.metaTitle"],"existingFields":{},"contentContext":{"type":"blog","technologies":["Rust","PostgreSQL","Redis"]}}
    });
    document["components"]["schemas"]["AiGenerationResponse"] = json!({
        "type":"object","additionalProperties":false,"required":["contentType","generationId","generatedFields","warnings","skippedFields","usage"],
        "properties":{
            "contentType":{"type":"string","enum":["blog","project"]},"generationId":{"type":"string","format":"uuid"},
            "generatedFields":{"type":"object","description":"Only requested CMS paths. Text values are strings; tags/technologies/features are string arrays; faqs are question/answer arrays; editorContent is validated Tiptap JSON. No persistence occurs."},
            "usage":{"anyOf":[{"$ref":"#/components/schemas/AiGenerationUsage"},{"type":"null"}],"description":"Provider-reported tokens and a server-calculated estimate, never model-generated values. Null if no provider call or metadata is unavailable."},
            "warnings":{"type":"array","items":{"type":"string"}},
            "skippedFields":{"type":"array","items":{"type":"object","required":["field","reason"],"properties":{"field":{"type":"string","enum":fields},"reason":{"type":"string"}}}}
        }
    });
    document["components"]["schemas"]["AiGenerationUsage"] = json!({
        "type":"object","additionalProperties":false,
        "required":["model","inputTokens","outputTokens","thinkingTokens","cachedInputTokens","totalTokens","estimatedCost"],
        "properties":{
            "model":{"type":"string"},
            "inputTokens":{"type":"integer","minimum":0,"description":"Prompt tokens, including the cached subset."},
            "outputTokens":{"type":"integer","minimum":0,"description":"Candidate tokens plus thinking tokens, billed at the output rate."},
            "thinkingTokens":{"type":"integer","minimum":0},
            "cachedInputTokens":{"type":"integer","minimum":0},
            "totalTokens":{"type":"integer","minimum":0},
            "estimatedCost":{"anyOf":[{"$ref":"#/components/schemas/AiGenerationCost"},{"type":"null"}],"description":"Token-only estimate, not an invoice. Null when the model is unpriced or token accounting is incomplete."}
        }
    });
    document["components"]["schemas"]["AiGenerationCost"] = json!({
        "type":"object","additionalProperties":false,
        "required":["currency","inputUsd","outputUsd","totalUsd","inputUsdPerMillion","outputUsdPerMillion","cachedInputUsdPerMillion"],
        "properties":{
            "currency":{"type":"string","enum":["USD"]},
            "inputUsd":{"type":"number","minimum":0},"outputUsd":{"type":"number","minimum":0},"totalUsd":{"type":"number","minimum":0},
            "inputUsdPerMillion":{"type":"number","minimum":0},"outputUsdPerMillion":{"type":"number","minimum":0},"cachedInputUsdPerMillion":{"type":"number","minimum":0}
        }
    });
    for name in ["AiGenerationRequest", "AiGenerationResponse"] {
        document["components"]["schemas"][name]["properties"]["contentType"]["enum"] =
            json!(["blog", "project", "case_study", "service"]);
    }
    let mut context = document["components"]["schemas"]["AiGenerationRequest"]["properties"]
        ["contentContext"]["oneOf"][1]
        .clone();
    context["properties"]["type"]["const"] = json!("case_study");
    if let Some(contexts) = document["components"]["schemas"]["AiGenerationRequest"]["properties"]
        ["contentContext"]["oneOf"]
        .as_array_mut()
    {
        contexts.push(context.clone());
        context["properties"]["type"]["const"] = json!("service");
        contexts.push(context);
    }
    document["paths"]["/api/v1/admin/ai/generate"] = json!({"post":{
        "tags":["AI content"],"summary":"Generate a reviewable blog/project/case study field patch without saving",
        "description":"Requires owner/admin role and the corresponding content-write permission. 256 KB request limit, per-admin quota, and bounded concurrency. Gemini credentials stay in Rust.",
        "security":[{"bearerAuth":[]}],
        "requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/AiGenerationRequest"}}}},
        "responses":{
            "200":{"description":"Validated unsaved content suggestions","content":{"application/json":{"schema":{"$ref":"#/components/schemas/AiGenerationResponse"}}}},
            "401":{"description":"Authentication required"},"403":{"description":"Administrator role and content-write permission required"},
            "422":{"description":"Invalid context, unsupported fields, or request too large"},
            "429":{"description":"Admin/provider quota reached or generation is busy"},
            "502":{"description":"Provider unavailable or invalid generated structure"},
            "503":{"description":"AI provider is not configured"},"504":{"description":"Generation timed out"}
        }
    }});
}
