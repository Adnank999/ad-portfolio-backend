use crate::common::{
    error::{AppError, AppResult},
    validation,
};
use axum::{
    extract::{Path, State},
    routing::{get, put},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use std::{collections::HashMap, sync::OnceLock};

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum Visual {
    Image {
        src: String,
        alt: String,
    },
    Shader {
        shader: String,
        version: u32,
        props: Value,
        #[serde(default)]
        poster: String,
        alt: String,
    },
}

pub struct Catalog {
    pub manifest: Value,
    validators: HashMap<(String, u32), jsonschema::Validator>,
}

static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();

pub fn catalog() -> AppResult<&'static Catalog> {
    CATALOG
        .get_or_init(|| compile_catalog(include_str!("../../../../assets/shaders.manifest.json")))
        .as_ref()
        .map_err(|_| AppError::Internal)
}

fn compile_catalog(raw: &str) -> Result<Catalog, String> {
    let manifest: Value = serde_json::from_str(raw).map_err(|_| "Invalid shader manifest")?;
    if manifest["formatVersion"] != 1 {
        return Err("Unsupported shader manifest".into());
    }
    let entries = manifest["shaders"].as_array().ok_or("Missing shaders")?;
    if entries.len() > 64 {
        return Err("Too many shaders".into());
    }
    let mut validators = HashMap::new();
    let mut ids = std::collections::HashSet::new();
    for entry in entries {
        let id = entry["id"].as_str().ok_or("Missing shader ID")?;
        let version = entry["version"]
            .as_u64()
            .filter(|value| *value > 0 && *value <= i32::MAX as u64)
            .ok_or("Invalid shader version")? as u32;
        if !ids.insert(id)
            || entry
                .get("defaultActive")
                .is_some_and(|value| !value.is_boolean())
            || !entry["contract"].as_str().is_some_and(|value| {
                value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
            || entry["schema"]["additionalProperties"] != false
        {
            return Err("Invalid shader contract".into());
        }
        let contract = json!({"id":entry["id"],"name":entry["name"],"version":entry["version"],"defaults":entry["defaults"],"schema":entry["schema"],"controls":entry["controls"]});
        let digest = hex::encode(Sha256::digest(
            serde_json::to_vec(&contract).map_err(|_| "Invalid contract JSON")?,
        ));
        if entry["contract"].as_str() != Some(digest.as_str()) {
            return Err(
                "Shader contract hash mismatch; regenerate from the frontend definitions".into(),
            );
        }
        let validator =
            jsonschema::validator_for(&entry["schema"]).map_err(|_| "Invalid shader schema")?;
        if !validator.is_valid(&entry["defaults"]) {
            return Err("Invalid shader defaults".into());
        }
        validators.insert((id.to_owned(), version), validator);
    }
    Ok(Catalog {
        manifest,
        validators,
    })
}

pub fn valid_slot(slot: &str) -> bool {
    matches!(
        slot,
        "hero" | "overview" | "spotlight" | "article-1" | "article-2" | "article-3"
    )
}

pub fn validate(slot: &str, value: &Value) -> AppResult<()> {
    if !valid_slot(slot) {
        return Err(AppError::Invalid("Invalid service visual slot".into()));
    }
    let visual: Visual = serde_json::from_value(value.clone())
        .map_err(|_| AppError::Invalid(format!("{slot}: invalid image/shader visual")))?;
    let (image, alt) = match &visual {
        Visual::Image { src, alt } => (src, alt),
        Visual::Shader {
            shader,
            version,
            props,
            poster,
            alt,
        } => {
            let validator = catalog()?
                .validators
                .get(&(shader.clone(), *version))
                .ok_or_else(|| {
                    AppError::Invalid(format!(
                        "{slot}: unsupported shader/version; deploy matching manifests"
                    ))
                })?;
            if serde_json::to_vec(props)
                .map_err(|_| AppError::Internal)?
                .len()
                > 16_384
                || !validator.is_valid(props)
            {
                return Err(AppError::Invalid(format!(
                    "{slot}: shader props do not match the deployed schema"
                )));
            }
            (poster, alt)
        }
    };
    validation::url(
        image,
        &format!("{slot} image/poster"),
        true,
        matches!(&visual, Visual::Shader { .. }),
    )?;
    validation::text(alt, &format!("{slot} visual description"), 1, 500)?;
    Ok(())
}

pub fn routes() -> Router<crate::common::state::AppState> {
    Router::new()
        .route("/shaders", get(read_catalog))
        .route("/shaders/{id}/activation", put(update_activation))
}

async fn read_catalog(
    State(state): State<crate::common::state::AppState>,
    Extension(actor): Extension<crate::domains::auth::Principal>,
) -> AppResult<Json<Value>> {
    actor.require("services:write")?;
    Ok(Json(activation_catalog(&state.pool, &actor).await?))
}

fn default_active(entry: &Value) -> bool {
    entry["defaultActive"].as_bool().unwrap_or(true)
}

fn activation_database_error(error: sqlx::Error) -> AppError {
    if matches!(&error, sqlx::Error::Database(database) if database.code().as_deref() == Some("42P01"))
    {
        tracing::error!(error = %error, "Shader activation schema is missing; apply pending backend migrations to the API database");
        return AppError::ShaderSchemaUnavailable;
    }
    error.into()
}

pub async fn activation_catalog(
    pool: &PgPool,
    actor: &crate::domains::auth::Principal,
) -> AppResult<Value> {
    let states = sqlx::query_as::<_, (String, i32, String, bool)>(
        "SELECT shader_id,version,contract,active FROM shader_activations",
    )
    .fetch_all(pool)
    .await
    .map_err(activation_database_error)?;
    let mut manifest = catalog()?.manifest.clone();
    manifest["canManage"] = json!(actor.require("services:publish").is_ok());
    for entry in manifest["shaders"]
        .as_array_mut()
        .ok_or(AppError::Internal)?
    {
        let active = states
            .iter()
            .find(|(id, version, contract, _)| {
                entry["id"] == *id && entry["version"] == *version && entry["contract"] == *contract
            })
            .map(|(_, _, _, active)| *active)
            .unwrap_or_else(|| default_active(entry));
        entry["active"] = json!(active);
    }
    Ok(manifest)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivationInput {
    version: u32,
    contract: String,
    active: bool,
}

async fn update_activation(
    State(state): State<crate::common::state::AppState>,
    Extension(actor): Extension<crate::domains::auth::Principal>,
    Path(id): Path<String>,
    Json(input): Json<ActivationInput>,
) -> AppResult<Json<Value>> {
    actor.require("services:publish")?;
    let entry = catalog()?.manifest["shaders"]
        .as_array()
        .ok_or(AppError::Internal)?
        .iter()
        .find(|entry| entry["id"] == id)
        .ok_or(AppError::NotFound)?;
    if entry["version"] != input.version || entry["contract"] != input.contract {
        return Err(AppError::Conflict(
            "Shader contract changed; reload the catalog before activation".into(),
        ));
    }
    sqlx::query("INSERT INTO shader_activations(shader_id,version,contract,active,updated_by) VALUES($1,$2,$3,$4,$5) ON CONFLICT(shader_id,version,contract) DO UPDATE SET active=excluded.active,updated_by=excluded.updated_by,updated_at=now()")
        .bind(&id).bind(input.version as i32).bind(&input.contract).bind(input.active).bind(actor.id)
        .execute(&state.pool).await.map_err(activation_database_error)?;
    Ok(Json(activation_catalog(&state.pool, &actor).await?))
}

pub async fn validate_assignments(
    transaction: &mut Transaction<'_, Postgres>,
    fields: &Value,
    previous: Option<&Value>,
) -> AppResult<()> {
    let Some(visuals) = fields["visuals"].as_object() else {
        return Ok(());
    };
    for (slot, visual) in visuals {
        if visual["type"] != "shader" {
            continue;
        }
        if previous
            .and_then(|fields| fields["visuals"].get(slot))
            .is_some_and(|old| {
                old["type"] == "shader"
                    && old["shader"] == visual["shader"]
                    && old["version"] == visual["version"]
            })
        {
            continue;
        }
        let entry = catalog()?.manifest["shaders"]
            .as_array()
            .ok_or(AppError::Internal)?
            .iter()
            .find(|entry| entry["id"] == visual["shader"] && entry["version"] == visual["version"])
            .ok_or_else(|| AppError::Invalid("Shader is not configured on this backend".into()))?;
        let active = sqlx::query_scalar::<_, bool>("SELECT active FROM shader_activations WHERE shader_id=$1 AND version=$2 AND contract=$3")
            .bind(entry["id"].as_str().ok_or(AppError::Internal)?)
            .bind(entry["version"].as_i64().ok_or(AppError::Internal)? as i32)
            .bind(entry["contract"].as_str().ok_or(AppError::Internal)?)
            .fetch_optional(&mut **transaction).await.map_err(activation_database_error)?.unwrap_or_else(|| default_active(entry));
        if !active {
            return Err(AppError::Invalid(format!(
                "{slot}: activate this shader in the admin shader library before assigning it"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deployed_shader_defaults_validate_for_every_slot() {
        for entry in catalog().unwrap().manifest["shaders"].as_array().unwrap() {
            for slot in [
                "hero",
                "overview",
                "spotlight",
                "article-1",
                "article-2",
                "article-3",
            ] {
                assert!(validate(slot, &json!({"type":"shader","shader":entry["id"],"version":entry["version"],"props":entry["defaults"],"poster":"/poster.webp","alt":"Shader fallback"})).is_ok());
            }
        }
    }

    #[test]
    fn rejects_unknown_versions_drafts_executable_props_and_unsafe_posters() {
        let defaults = catalog().unwrap().manifest["shaders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == "orb")
            .unwrap()["defaults"]
            .clone();
        let mut visual = json!({"type":"shader","shader":"orb","version":1,"props":defaults,"poster":"/poster.webp","alt":"Orb poster"});
        assert!(validate("hero", &visual).is_ok());
        for (key, invalid) in [
            ("shader", json!("draft-effect")),
            ("version", json!(999)),
            ("poster", json!("javascript:alert(1)")),
            ("poster", json!(null)),
            ("alt", json!("")),
        ] {
            let mut candidate = visual.clone();
            candidate[key] = invalid;
            assert!(validate("hero", &candidate).is_err());
        }
        visual["props"]["speed"] = json!(1000);
        assert!(validate("hero", &visual).is_err());
        visual["props"] = defaults;
        visual["props"]["onClick"] = json!("alert(1)");
        assert!(validate("hero", &visual).is_err());
        assert!(validate(
            "unknown",
            &json!({"type":"image","src":"/image.webp","alt":"Image"})
        )
        .is_err());
        assert!(validate(
            "hero",
            &json!({"type":"image","src":"/image.webp","alt":"Image","shader":"orb"})
        )
        .is_err());
    }

    #[test]
    fn shader_backgrounds_allow_missing_or_empty_posters_but_images_still_require_urls() {
        let entry = &catalog().unwrap().manifest["shaders"][0];
        let mut visual = json!({"type":"shader","shader":entry["id"],"version":entry["version"],"props":entry["defaults"],"alt":"Crimson background effect"});
        assert!(validate("hero", &visual).is_ok());
        visual["poster"] = json!("");
        assert!(validate("overview", &visual).is_ok());
        assert!(validate("hero", &json!({"type":"image","src":"","alt":"Image"})).is_err());
        assert!(validate("hero", &json!({"type":"image","alt":"Image"})).is_err());
    }

    #[test]
    fn invalid_manifests_fail_closed() {
        assert!(compile_catalog("{}").is_err());
        let mut manifest = catalog().unwrap().manifest.clone();
        let duplicate = manifest["shaders"][0].clone();
        manifest["shaders"].as_array_mut().unwrap().push(duplicate);
        assert!(compile_catalog(&manifest.to_string()).is_err());
    }

    #[test]
    fn configured_drafts_default_inactive_without_changing_legacy_contracts() {
        assert!(default_active(&json!({})));
        assert!(default_active(&json!({"defaultActive":true})));
        assert!(!default_active(&json!({"defaultActive":false})));
        let mut manifest = catalog().unwrap().manifest.clone();
        manifest["shaders"][0]["defaultActive"] = json!(false);
        assert!(compile_catalog(&manifest.to_string()).is_ok());
        manifest["shaders"][0]["defaultActive"] = json!("false");
        assert!(compile_catalog(&manifest.to_string()).is_err());
    }

    #[test]
    fn installed_shader_contracts_reject_untrusted_props_and_out_of_range_controls() {
        let catalog = catalog().unwrap();
        for entry in catalog.manifest["shaders"].as_array().unwrap() {
            let validator = &catalog.validators[&(
                entry["id"].as_str().unwrap().to_owned(),
                entry["version"].as_u64().unwrap() as u32,
            )];
            let mut invalid = entry["defaults"].clone();
            invalid["onClick"] = json!("untrusted");
            assert!(!validator.is_valid(&invalid));
            for control in entry["controls"].as_array().unwrap() {
                let key = control["key"].as_str().unwrap();
                let mut invalid = entry["defaults"].clone();
                invalid[key] = match control["kind"].as_str().unwrap() {
                    "slider" => json!(control["max"].as_f64().unwrap() + 1.0),
                    "toggle" => json!("true"),
                    "color" => json!("url(javascript:alert(1))"),
                    "color-list" => json!(["not-a-color", "not-a-color"]),
                    "select" => json!("unknown-choice"),
                    _ => panic!("Unexpected shader control"),
                };
                assert!(!validator.is_valid(&invalid), "{}: {key}", entry["id"]);
            }
        }
    }
}
