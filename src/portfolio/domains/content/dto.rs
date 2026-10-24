use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Blog,
    Project,
    CaseStudy,
    Service,
}
impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blog => "blog",
            Self::Project => "project",
            Self::CaseStudy => "case_study",
            Self::Service => "service",
        }
    }
    pub fn plural(self) -> &'static str {
        match self {
            Self::Blog => "blogs",
            Self::Project => "projects",
            Self::CaseStudy => "case-studies",
            Self::Service => "services",
        }
    }
    pub fn frontend_path(self) -> &'static str {
        match self {
            Self::Blog => "blog",
            Self::Project => "projects",
            Self::CaseStudy => "case-studies",
            Self::Service => "services",
        }
    }
    pub fn permission(self, action: &str) -> String {
        format!("{}:{action}", self.plural())
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Draft,
    Published,
    Archived,
}
impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
            Self::Archived => "archived",
        }
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Seo {
    pub structured_data: Option<Value>,
    pub meta_title: Option<String>,
    pub meta_description: Option<String>,
    pub canonical_url: Option<String>,
    pub og_title: Option<String>,
    pub og_description: Option<String>,
    pub og_image: Option<String>,
    pub og_image_alt: Option<String>,
    pub twitter_title: Option<String>,
    pub twitter_description: Option<String>,
    pub twitter_image: Option<String>,
    pub no_index: bool,
    pub no_follow: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Faq {
    pub question: String,
    pub answer: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentWrite {
    #[serde(default)]
    pub asset_ids: Vec<uuid::Uuid>,
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub status: Status,
    pub published_at: Option<DateTime<Utc>>,
    pub fields: Value,
    pub editor_content: Option<Value>,
    #[serde(default)]
    pub seo: Seo,
    #[serde(default)]
    pub faqs: Vec<Faq>,
    pub version: Option<i32>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Item {
    pub title: String,
    pub text: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentTable {
    pub caption: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlogSection {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub paragraphs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cards: Option<Vec<Item>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<ContentTable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callout: Option<Item>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BlogFields {
    pub category: String,
    pub cover: String,
    pub cover_alt: String,
    pub social_image: String,
    #[serde(default)]
    pub intro: Vec<String>,
    #[serde(default)]
    pub takeaways: Vec<String>,
    #[serde(default)]
    pub sections: Vec<BlogSection>,
    #[serde(default)]
    pub tags: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectImage {
    pub url: String,
    pub alt: String,
    #[serde(default)]
    pub caption: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectFields {
    pub image: String,
    pub image_alt: String,
    #[serde(default)]
    pub gallery: Vec<ProjectImage>,
    pub category: String,
    pub year: String,
    #[serde(default)]
    pub technologies: Vec<String>,
    #[serde(default)]
    pub github_url: String,
    #[serde(default)]
    pub live_url: String,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub overview: String,
    #[serde(default)]
    pub problem: String,
    #[serde(default)]
    pub solution: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub technical: String,
    #[serde(default)]
    pub challenges: String,
    #[serde(default)]
    pub results: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaseStudyFields {
    pub industry: String,
    pub study_type: String,
    pub image: String,
    pub image_alt: String,
    #[serde(default)]
    pub gallery: Vec<ProjectImage>,
    pub context: String,
    pub problem: String,
    pub solution: String,
    #[serde(default)]
    pub steps: Vec<Item>,
    #[serde(default)]
    pub technologies: Vec<String>,
    #[serde(default)]
    pub outcomes: Vec<Item>,
    #[serde(default)]
    pub lessons: Vec<String>,
    #[serde(default)]
    pub project_slug: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub category: Option<String>,
    pub tag: Option<String>,
    pub featured: Option<bool>,
    pub q: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionQuery {
    pub version: i32,
}
