# Service CMS

Services use the versioned `content_items` store with `kind=service`, the same authentication, optimistic versions, slug history, publishing and soft deletion as case studies.

## Setup

From `Backend`, run `cargo run --bin portfolio-api -- migrate`, then `cargo run --bin portfolio-api -- import-services`, then restart the API. An active owner must already exist. The transaction imports all 13 services from `examples/services.json`, including every existing section, FAQ and navbar entry. Reserved slugs are skipped, so repeat imports preserve edits and deletions. To regenerate defaults from the original frontend content, run `bun run scripts/export-service-seed.ts` in the frontend.

On Windows, if the running API locks its executable, `cargo run --example import_services` applies migrations and imports through a separate maintenance executable. Restart the API afterward to load the new routes.

## Editing and visibility

Use `/admin/services` for the collection and quick Service/Navbar switches. The editor supports creation and changes to copy, every structured section, FAQs, navbar title/description/group/order/badges and artwork fallback. `enabled=false` removes the service from public collections, direct URLs and sitemap. `navEnabled=false` hides it from desktop/mobile menus while the enabled published page remains reachable. Draft, archived, future and deleted services never appear publicly. The services index and related-service links use the public database collection. No static fallback resurrects disabled content.

Public APIs are `GET /api/v1/services` and `GET /api/v1/services/{slug}`. Authenticated CRUD uses `/api/v1/admin/services` and `/api/v1/admin/services/{id}`. Owners/admins receive `services:write`, `services:publish`, `services:delete`; editors can edit drafts. Changes to published content require publish permission, including visibility switches.

## Media

Hero, overview, spotlight and three article slots accept images or videos. Empty slots preserve their original SVG illustration. Image uploads allow JPEG/PNG/WebP/GIF/AVIF up to 8 MB; service video uploads allow MP4/WebM up to 64 MB. The existing authenticated R2 endpoint validates file signatures and streamed size, keeps credentials server-side and requires service-write permission for video. Configure the existing R2 environment settings. Native videos have controls, optional posters and captions, descriptions and no autoplay. Existing hosted URLs are also supported.

## SEO and AI

Metadata, canonical URLs, Open Graph, Twitter, robots controls and custom schema.org JSON-LD use the shared editor. Server-rendered heads use database values. Automatic JSON-LD includes Service, breadcrumbs and the visible FAQs; the sitemap excludes disabled/noindex records. Slug changes redirect with HTTP 308.

AI uses `contentType=service` and matching context. It can propose core fields, navbar title/description, all structured page sections, FAQs and SEO. Existing source text supplies context. Media, visibility, publishing, category, artwork and badges are never model-editable. Review individual proposals, confirm replacements, then explicitly Save. AI credentials and quotas remain in the Rust API. Tests use mocked provider data.
