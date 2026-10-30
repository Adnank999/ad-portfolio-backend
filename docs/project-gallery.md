# Project image galleries

Projects retain their primary `fields.image` / `fields.imageAlt` cover and an optional ordered `fields.gallery` array. Writes now require `fields.thumbnailUrl` for terrain overview previews. `fields.featuredImageUrl` selects a cover/gallery image for the detail hero and defaults to the cover. Existing records remain readable with cover fallbacks; editors must set a thumbnail before saving older records. These settings use the existing PostgreSQL JSONB fields, so no migration or new table is required.

## Upload and save

1. Open Admin → Projects → create/edit → Project gallery → Upload images.
2. Select multiple still JPEG, PNG or WebP files, up to 20 MiB per original. A project supports up to 20 gallery images.
3. The shared browser conversion resizes/encodes WebP. With direct uploads enabled, the authenticated backend presigns a direct browser PUT to private R2 staging, verifies completion and publishes a ready asset. Asset IDs are included when saving. The legacy server upload remains available when direct uploads are disabled.
4. Successful uploads remain available even if another file fails. Retry only failed files. Enter required alt text, optional captions, and arrange images using the up/down buttons.
5. Set **Terrain thumbnail URL**, or upload through that field. Select the cover or one gallery image using **Use as featured image**. Selecting another replaces the choice; reordering preserves it and removing the chosen image returns to the cover. The existing **Featured project** flag is separate.
6. Save Changes persists these fields through Rust `POST /api/v1/admin/projects` or `PUT /api/v1/admin/projects/{id}`. Updating requires the full content payload and current version. Uploading alone does not save/publish a project. Saving is blocked while uploads are running.

For verified direct uploads, R2 credentials remain in the **backend environment**. Thumbnail uploads reuse the project-gallery purpose and register their verified asset ID automatically. New managed R2 URLs still require verified references; pasting a URL does not bypass that check. No new R2 credentials or bucket configuration are introduced.

## API contract

Include this within the normal project `fields` object; retain all other project fields and the primary image:

```json
{
  "thumbnailUrl": "https://your-public-r2-domain.example/portfolio/2026-10/small-preview.webp",
  "featuredImageUrl": "/images/project-checkout.webp",
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

Public Work pages load published database projects through the public API. Terrain overview portals and cards use `thumbnailUrl`; opening details loads the full featured texture on demand, and the native fallback uses the same featured image/alt text. The API preserves the gallery and its ordering. Deploy frontend and backend together for the new write contract.

Removing an item from a gallery only removes its project reference after Save Changes; it does **not** delete the R2 object. Other content may reference the same object. Failed/discarded drafts can leave unused uploads, just as the existing single-image uploader does.
