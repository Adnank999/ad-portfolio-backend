# Project image galleries

Projects retain their primary `fields.image` / `fields.imageAlt` cover and now support an optional ordered `fields.gallery` array. Existing projects and clients remain valid; an omitted gallery defaults to an empty array on save. Gallery data uses the existing PostgreSQL JSONB content fields, so no migration or new table is required.

## Upload and save

1. Open Admin → Projects → create/edit → Project gallery → Upload images.
2. Select multiple JPEG, PNG, WebP, GIF, or AVIF files. A project supports up to 20 gallery images, each at most 8 MiB.
3. Files upload sequentially through the existing authenticated TanStack Start server endpoint `POST /api/admin/media`. It retains same-origin checks, server-side authentication/permissions, bounded request reads, MIME/signature validation, and private R2 credentials. There is no unauthenticated or direct-browser R2 upload, nor a new bulk endpoint that buffers every file at once.
4. Successful uploads remain available even if another file fails. Retry only failed files. Enter required alt text, optional captions, and arrange images using the up/down buttons.
5. Save Changes persists the gallery through the existing Rust `POST /api/v1/admin/projects` or `PUT /api/v1/admin/projects/{id}` endpoint. Updating requires the normal full content payload and current version. Uploading alone does not save or publish a project. Saving is blocked while a gallery batch is uploading.

R2 configuration remains in the **frontend server environment**, where the existing storage integration lives: `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_BUCKET`, and `R2_PUBLIC_URL`. No new credentials are needed and none are sent to the browser.

## API contract

Include this within the normal project `fields` object; retain all other project fields and the primary image:

```json
{
  "gallery": [
    {
      "url": "https://your-public-r2-domain.example/portfolio/2026-10/uploaded-search.webp",
      "alt": "Hotel search results with date and price filters",
      "caption": "Search and filtering"
    },
    {
      "url": "/images/project-checkout.webp",
      "alt": "The booking checkout form",
      "caption": ""
    }
  ]
}
```

The Rust API validates a maximum of 20 entries, unique image URLs (HTTP(S) without credentials or safe root-relative paths, up to 2048 characters), alt text of 1–500 characters, and captions of at most 1000 characters. Unsupported fields and unsafe URLs are rejected. Array order is the display order. Set `gallery: []` to clear a gallery without changing the cover. Blog schemas are unchanged. AI generation does not generate or replace galleries.

Admin detail responses contain `fields.gallery`. Published public project responses from `GET /api/v1/projects/{slug}` expose `gallery` at the top level, consistent with other public project fields. Existing unsaved legacy rows can omit it; frontend consumers should use `project.gallery ?? []`.

The current public Work pages still read `src/data/projects.ts`; this change makes gallery data available through the API and admin, without replacing that existing static public-data source. When connecting those pages, render the returned images with their saved alt text and captions, preserving array order and lazy-loading non-cover images.

Removing an item from a gallery only removes its project reference after Save Changes; it does **not** delete the R2 object. Other content may reference the same object. Failed/discarded drafts can leave unused uploads, just as the existing single-image uploader does.
