use serde_json::{json, Value};

pub fn extend(document: &mut Value) {
    let purposes = [
        "blog_cover",
        "blog_inline",
        "project_cover",
        "project_gallery",
        "project_inline",
        "case_study_cover",
        "case_study_gallery",
        "case_study_inline",
        "service_image",
    ];
    document["components"]["schemas"]["ImagePresign"] = json!({
        "type":"object","additionalProperties":false,
        "required":["purpose","originalName","contentType","sizeBytes","width","height"],
        "properties":{
            "purpose":{"type":"string","enum":purposes},"originalName":{"type":"string","minLength":1,"maxLength":255},
            "contentType":{"type":"string","const":"image/webp"},"sizeBytes":{"type":"integer","minimum":1,"maximum":2097152},
            "width":{"type":"integer","minimum":1,"maximum":1920},"height":{"type":"integer","minimum":1,"maximum":1600}
        }
    });
    for name in [
        "BlogWrite",
        "ProjectWrite",
        "CaseStudyWrite",
        "ServiceWrite",
    ] {
        if document["components"]["schemas"][name].is_object() {
            document["components"]["schemas"][name]["properties"]["assetIds"] = json!({"type":"array","maxItems":100,"items":{"type":"string","format":"uuid"},"description":"Ready media IDs; existing URL fields remain display/compatibility fields."});
        }
    }
    document["paths"]["/api/v1/admin/uploads/policy"] = json!({"get":{"summary":"Read direct-image rollout availability","security":[{"bearerAuth":[]}],"responses":{"200":{"description":"Enabled flag for authenticated image editors"},"401":{"description":"Authentication required"},"403":{"description":"Image write permission required"}}}});
    document["paths"]["/api/v1/admin/uploads/presign"] = json!({"post":{"summary":"Sign a private write-once WebP PUT","security":[{"bearerAuth":[]}],"requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/ImagePresign"}}}},"responses":{"201":{"description":"uploadId, opaque uploadUrl, method PUT, exact headers and expiresAt"},"422":{"description":"Purpose, dimensions, MIME or byte policy rejected"},"429":{"description":"Actor signing/pending quota exceeded"},"503":{"description":"Direct uploads disabled or unconfigured"}}}});
    document["paths"]["/api/v1/admin/uploads/{id}/complete"] = json!({"post":{"summary":"Verify owned WebP, publish immutable image and return a ready asset idempotently","security":[{"bearerAuth":[]}],"parameters":[{"in":"path","name":"id","required":true,"schema":{"type":"string","format":"uuid"}}],"responses":{"200":{"description":"Canonical ready assetId, publicUrl, key, MIME, bytes, width and height"},"404":{"description":"Unknown upload or wrong owner"},"409":{"description":"Bytes not ready or upload no longer pending"},"410":{"description":"Upload expired"},"422":{"description":"Real image verification failed"},"502":{"description":"Retryable R2 failure"}}}});
    document["paths"]["/api/v1/admin/uploads/{id}"] = json!({"delete":{"summary":"Abandon an owned pending upload","security":[{"bearerAuth":[]}],"parameters":[{"in":"path","name":"id","required":true,"schema":{"type":"string","format":"uuid"}}],"responses":{"204":{"description":"Abandoned; staging cleanup deferred until signature expiry"},"409":{"description":"Upload cannot be abandoned"}}}});
}
