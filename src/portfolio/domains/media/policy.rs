use crate::common::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    BlogCover,
    BlogInline,
    ProjectCover,
    ProjectGallery,
    ProjectInline,
    CaseStudyCover,
    CaseStudyGallery,
    CaseStudyInline,
    ServiceImage,
}

impl Purpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BlogCover => "blog_cover",
            Self::BlogInline => "blog_inline",
            Self::ProjectCover => "project_cover",
            Self::ProjectGallery => "project_gallery",
            Self::ProjectInline => "project_inline",
            Self::CaseStudyCover => "case_study_cover",
            Self::CaseStudyGallery => "case_study_gallery",
            Self::CaseStudyInline => "case_study_inline",
            Self::ServiceImage => "service_image",
        }
    }
    pub fn kind(self) -> &'static str {
        match self {
            Self::BlogCover | Self::BlogInline => "blogs",
            Self::ProjectCover | Self::ProjectGallery | Self::ProjectInline => "projects",
            Self::CaseStudyCover | Self::CaseStudyGallery | Self::CaseStudyInline => "case-studies",
            Self::ServiceImage => "services",
        }
    }
    pub fn cover(self) -> bool {
        matches!(
            self,
            Self::BlogCover | Self::ProjectCover | Self::CaseStudyCover
        )
    }
    pub fn validate(self, size: i64, width: u32, height: u32) -> AppResult<()> {
        let maximum = if self.cover() {
            (1920, 1080)
        } else {
            (1600, 1600)
        };
        if size <= 0
            || size > 2 * 1024 * 1024
            || width == 0
            || height == 0
            || width > maximum.0
            || height > maximum.1
            || (self.cover() && (i64::from(width) * 9 - i64::from(height) * 16).abs() > 16)
        {
            return Err(AppError::Invalid(
                "Image size or dimensions do not match the selected purpose".into(),
            ));
        }
        Ok(())
    }
}
