use serde_json::{json, Value};

pub fn extend(document: &mut Value) {
    fn service_names(value: &mut Value) {
        match value {
            Value::String(text) => {
                *text = text
                    .replace("case-studies", "services")
                    .replace("CaseStudyWrite", "ServiceWrite")
                    .replace("case study", "service")
            }
            Value::Array(values) => values.iter_mut().for_each(service_names),
            Value::Object(values) => values.values_mut().for_each(service_names),
            _ => {}
        }
    }
    for suffix in ["", "/{slug}"] {
        let mut route = document["paths"][format!("/api/v1/case-studies{suffix}")].clone();
        service_names(&mut route);
        document["paths"][format!("/api/v1/services{suffix}")] = route;
    }
    for suffix in ["", "/{id}"] {
        let mut route = document["paths"][format!("/api/v1/admin/case-studies{suffix}")].clone();
        service_names(&mut route);
        document["paths"][format!("/api/v1/admin/services{suffix}")] = route;
    }
    let mut schema = document["components"]["schemas"]["CaseStudyWrite"].clone();
    schema["properties"]["editorContent"] =
        json!({"type":"null","description":"Services use pageContent sections."});
    schema["properties"]["fields"] = json!({
        "type":"object","additionalProperties":false,
        "required":["category","artwork","enabled","navEnabled","navTitle","navDescription","navGroup","navOrder","navVisual","navPopular","navAi","pageContent"],
        "properties":{
            "category":{"type":"string","maxLength":100},
            "artwork":{"type":"string","enum":["web","software","cloud","data","automation","systems","growth"]},
            "enabled":{"type":"boolean"},"navEnabled":{"type":"boolean"},
            "navTitle":{"type":"string","maxLength":200},"navDescription":{"type":"string","maxLength":1000},
            "navGroup":{"type":"string","maxLength":100},"navOrder":{"type":"integer","minimum":0,"maximum":10000},
            "navVisual":{"type":"string","enum":["orbit","cube","signal","device","grid"]},
            "navPopular":{"type":"boolean"},"navAi":{"type":"boolean"},
            "pageContent":super::services::page_schema().unwrap_or_else(|_| json!({"type":"object"})),
            "visuals":{"type":"object","maxProperties":6,"propertyNames":{"enum":["hero","overview","spotlight","article-1","article-2","article-3"]},"additionalProperties":{"$ref":"#/components/schemas/ServiceVisual"}},
            "media":{"type":"array","maxItems":6,"items":{"type":"object","additionalProperties":false,"required":["slot","kind","url","alt"],"properties":{
                "slot":{"type":"string","enum":["hero","overview","spotlight","article-1","article-2","article-3"]},
                "kind":{"type":"string","enum":["image","video"]},"url":{"type":"string"},"alt":{"type":"string","maxLength":500},"poster":{"type":"string"},"caption":{"type":"string","maxLength":1000}
            }}}
        }
    });
    document["components"]["schemas"]["ServiceWrite"] = schema;
    document["components"]["schemas"]["ServiceVisual"] = json!({"oneOf":[
        {"type":"object","additionalProperties":false,"required":["type","src","alt"],"properties":{"type":{"const":"image"},"src":{"type":"string","maxLength":2048},"alt":{"type":"string","minLength":1,"maxLength":500}}},
        {"type":"object","additionalProperties":false,"required":["type","shader","version","props","alt"],"properties":{"type":{"const":"shader"},"shader":{"type":"string"},"version":{"type":"integer","minimum":1},"props":{"type":"object","description":"Validated against the deployed shader manifest"},"poster":{"type":"string","maxLength":2048,"description":"Optional legacy image metadata; shaders render over the page background"},"alt":{"type":"string","minLength":1,"maxLength":500}}}
    ]});
    document["paths"]["/api/v1/admin/shaders"] = json!({"get":{"summary":"Configured shader contracts and persisted activation; requires services:write","security":[{"bearerAuth":[]}],"responses":{"200":{"description":"Manifest with active state per shader contract and canManage for services:publish"},"401":{"description":"Authentication required"},"403":{"description":"Permission required"}}}});
    document["paths"]["/api/v1/admin/shaders/{id}/activation"] = json!({"put":{"summary":"Activate/deactivate a configured shader; requires services:publish. Existing assignments remain usable.","security":[{"bearerAuth":[]}],"parameters":[{"name":"id","in":"path","required":true,"schema":{"type":"string"}}],"requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","additionalProperties":false,"required":["version","contract","active"],"properties":{"version":{"type":"integer","minimum":1,"maximum":2147483647},"contract":{"type":"string","pattern":"^[a-f0-9]{64}$"},"active":{"type":"boolean"}}}}}},"responses":{"200":{"description":"Updated activation catalog"},"401":{"description":"Authentication required"},"403":{"description":"Publishing permission required"},"404":{"description":"Unconfigured shader"},"409":{"description":"Stale shader contract"},"422":{"description":"Invalid activation request"}}}});
}
