use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Blog,
    Project,
    #[serde(rename = "case_study")]
    CaseStudy,
    Service,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Create,
    Regenerate,
    FillMissing,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Professional,
    Technical,
    Conversational,
    Storytelling,
    Educational,
    CaseStudy,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Audience {
    GeneralReaders,
    SoftwareDevelopers,
    SeniorEngineers,
    TechnicalRecruiters,
    BusinessOwners,
    PotentialClients,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Length {
    Short,
    Medium,
    Long,
    InDepth,
}

macro_rules! fields {
    ($($variant:ident => $key:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Field { $(#[serde(rename = $key)] $variant),+ }
        impl Field {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn key(self) -> &'static str { match self { $(Self::$variant => $key),+ } }
        }
    };
}
fields! {
    Title => "title", Slug => "slug", Description => "description", EditorContent => "editorContent", Faqs => "faqs",
    Category => "fields.category", Tags => "fields.tags", CoverAlt => "fields.coverAlt", SocialImage => "fields.socialImage",
    ImageAlt => "fields.imageAlt", Year => "fields.year", Technologies => "fields.technologies",
    Overview => "fields.overview", Problem => "fields.problem", Solution => "fields.solution", Technical => "fields.technical",
    Challenges => "fields.challenges", Results => "fields.results", Features => "fields.features",
    Context => "fields.context", Lessons => "fields.lessons",
    PageContent => "fields.pageContent", NavTitle => "fields.navTitle", NavDescription => "fields.navDescription",
    MetaTitle => "seo.metaTitle", MetaDescription => "seo.metaDescription", CanonicalUrl => "seo.canonicalUrl",
    OgTitle => "seo.ogTitle", OgDescription => "seo.ogDescription", OgImage => "seo.ogImage", OgImageAlt => "seo.ogImageAlt",
    TwitterTitle => "seo.twitterTitle", TwitterDescription => "seo.twitterDescription", TwitterImage => "seo.twitterImage",
}
impl Field {
    pub fn allowed(self, kind: ContentType) -> bool {
        match self {
            Self::Category => matches!(kind, ContentType::Blog | ContentType::Project),
            Self::PageContent | Self::NavTitle | Self::NavDescription => {
                kind == ContentType::Service
            }
            Self::EditorContent => kind != ContentType::Service,
            Self::Tags | Self::CoverAlt | Self::SocialImage => kind == ContentType::Blog,
            Self::ImageAlt | Self::Technologies | Self::Problem | Self::Solution => {
                matches!(kind, ContentType::Project | ContentType::CaseStudy)
            }
            Self::Context | Self::Lessons => kind == ContentType::CaseStudy,
            Self::Year
            | Self::Overview
            | Self::Technical
            | Self::Challenges
            | Self::Results
            | Self::Features => kind == ContentType::Project,
            _ => true,
        }
    }
    pub fn derived(self) -> bool {
        matches!(
            self,
            Self::Year
                | Self::CanonicalUrl
                | Self::OgImage
                | Self::TwitterImage
                | Self::SocialImage
        )
    }
    pub fn maximum(self) -> usize {
        match self {
            Self::Title | Self::NavTitle | Self::MetaTitle | Self::OgTitle | Self::TwitterTitle => {
                200
            }
            Self::Slug => 120,
            Self::Category => 100,
            Self::CoverAlt | Self::ImageAlt | Self::OgImageAlt => 500,
            Self::Context
            | Self::Overview
            | Self::Problem
            | Self::Solution
            | Self::Technical
            | Self::Challenges
            | Self::Results => 20_000,
            _ => 1000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlogContext {
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub argument: String,
    #[serde(default)]
    pub concepts: String,
    #[serde(default)]
    pub technologies: Vec<String>,
    #[serde(default)]
    pub examples: String,
    #[serde(default)]
    pub references: String,
    #[serde(default)]
    pub structure: String,
    #[serde(default)]
    pub image_description: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectContext {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub project_type: String,
    #[serde(default)]
    pub technologies: Vec<String>,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub problem: String,
    #[serde(default)]
    pub solution: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub challenges: String,
    #[serde(default)]
    pub outcomes: String,
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub year: String,
    #[serde(default)]
    pub image_description: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContentContext {
    Blog(BlogContext),
    Project(ProjectContext),
    #[serde(rename = "case_study")]
    CaseStudy(ProjectContext),
    Service(ProjectContext),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerateRequest {
    pub content_type: ContentType,
    pub mode: Mode,
    pub context: String,
    #[serde(default)]
    pub additional_instructions: String,
    pub tone: Tone,
    pub audience: Audience,
    pub length: Length,
    pub language: String,
    #[serde(default)]
    pub seo_keywords: Vec<String>,
    pub target_fields: Vec<Field>,
    #[serde(default)]
    pub existing_fields: BTreeMap<String, Value>,
    pub content_context: ContentContext,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDraft {
    pub generated_fields: BTreeMap<Field, Value>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkippedField {
    pub field: Field,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerateResponse {
    pub content_type: ContentType,
    pub generation_id: Uuid,
    pub generated_fields: BTreeMap<Field, Value>,
    pub warnings: Vec<String>,
    pub skipped_fields: Vec<SkippedField>,
    #[serde(default)]
    pub usage: Option<GenerationUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationUsage {
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub thinking_tokens: u32,
    pub cached_input_tokens: u32,
    pub total_tokens: u32,
    pub estimated_cost: Option<GenerationCost>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationCost {
    pub currency: String,
    pub input_usd: f64,
    pub output_usd: f64,
    pub total_usd: f64,
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
    pub cached_input_usd_per_million: f64,
}
