# Case study CMS

Case studies use the existing versioned content store with `kind=case_study`. Public routes expose only published, non-deleted, non-future records. Old slugs redirect permanently; deleted slugs stay reserved. The shared sitemap includes indexed case studies.

## Set up and import

From `Backend`:

```powershell
cargo run --bin portfolio-api -- migrate
cargo run --bin portfolio-api -- import-case-studies
cargo run --bin portfolio-api -- serve
```

`examples/case-studies.json` contains all 13 existing frontend studies, preserving their copy, evidence labels, steps, outcomes, technology lists, and FAQs. Import runs in a transaction, assigns an existing active owner, and skips every previously reserved slug. It never overwrites edited content, restores deleted studies, or duplicates entries. Bootstrap an owner first if none exists. Review the import file before using it against a production database.

Portfolio entries retain their existing raster cover URLs. Concepts without supplied photos start with `/images/case-studies-cover.png`; replace it using the admin cover uploader. No SVG artwork is used as a study cover. Optional seed regeneration prints JSON from `bun scripts/export-case-study-seed.ts` in the frontend; it does not import or overwrite a database automatically.

## Routes and fields

- Public: `GET /api/v1/case-studies` and `GET /api/v1/case-studies/{slug}`.
- Admin: `GET|POST /api/v1/admin/case-studies`, `GET|PUT|DELETE /api/v1/admin/case-studies/{id}`.
- Frontend workspace: `/admin/case-studies`, `/admin/case-studies/new`, `/admin/case-studies/{id}`.
- `fields`: `industry`, `studyType` (`portfolio` or `concept`), `image`, `imageAlt`, ordered `gallery`, `context`, `problem`, `solution`, `steps`, `technologies`, `outcomes`, `lessons`, optional `projectSlug`.
- Industry filters use the shared `category` query parameter. `industry` must be one of the ten existing industry slugs.
- Publication, slug history, search, pagination, stale-version conflicts and soft deletion follow the blog/project API. Pass the current `version` on updates and deletes.
- Owners and admins receive `case-studies:write`, `case-studies:publish`, and `case-studies:delete`; editors receive draft-write permission. Published edits require publish permission. Read-only users retain `content:read` only.

## Photos, SEO and AI

The editor reuses authenticated R2 upload infrastructure for the cover, rich content, and up to 20 gallery photos. Gallery alt text is required; captions and ordering are editable. Uploads do not publish content. Storage credentials stay server-side; existing R2 configuration is required.

SEO fields include metadata, canonical, Open Graph, Twitter, and robots settings. Visible FAQs generate FAQPage JSON-LD automatically. Optional `seo.structuredData` adds a bounded schema.org object or graph (64 KB, maximum depth 16). The frontend serializes JSON-LD with HTML-sensitive escaping. Rich content is sanitized by the existing backend renderer and replaces the structured narrative when edited; FAQs and gallery remain separate.

Generate with AI uses `contentType=case_study` and `contentContext.type=case_study`. It supports title, slug, summary, rich content, context, problem, solution, lessons, technologies, trustworthy alt text, FAQs and SEO. Industry, study type, photos, gallery, publication controls and custom JSON-LD are not model-editable. The same owner/admin checks, bounded provider calls, quotas, field preview, conflict detection and explicit Save action used for blogs apply. No live AI call is made by imports or tests.
