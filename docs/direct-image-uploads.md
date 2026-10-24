# Direct WebP admin images

## Architecture and compatibility

The previous image path sent raw bytes to the TanStack `/api/admin/media` server route, whose Node S3 client wrote R2 objects. The Rust API did not previously own image uploads. The new path is shared by blogs, projects, case studies, service image slots, project/case-study galleries, and rich-text images:

1. The browser validates a still JPEG/PNG/WebP source, applies a purpose-specific resize/crop, and encodes actual WebP.
2. The same-origin authenticated TanStack route forwards small JSON to the private Rust API, preserving the existing access cookie, bearer forwarding, and Origin checks.
3. Rust allocates a database session and signs a write-once PUT to a private R2 staging bucket.
4. Only the browser sends the image-sized PUT to R2. No application cookies, bearer headers, server action, or API proxy is involved in that transfer.
5. Rust reads the staged object back from R2 under an ETag condition, checks metadata, fully decodes the WebP with bounded bytes/pixels, rejects animation, and conditionally copies it to an immutable public key.
6. The browser receives `assetId` plus the canonical public URL. Content Save submits `assetIds`; Rust checks ready state, owner/admin permission, content kind and placement, and records relational references atomically with the existing content save.

Normal browser-to-server image bytes bypass both application servers. Verification intentionally downloads the converted image from R2 to Rust, at most 2 MiB per image. Two simultaneous validation slots cap decoder memory; this is stronger validation than a MIME or header-only check.

Migration `202610250001_direct_image_uploads.sql` adds `image_uploads`, `media_assets`, and `content_media_assets`. Existing `content_items.fields` URL fields and editor documents remain readable. No image columns, old objects, videos, GLB models, documents, or database volumes are removed. The original media route remains for videos and explicit image rollback, but rejects raw image uploads while the frontend direct-upload flag is enabled.

## Implementation files

| Area | Files and responsibility |
| --- | --- |
| Database | `migrations/202610250001_direct_image_uploads.sql`: additive upload sessions, ready assets, relational content references and cleanup indexes |
| Backend wiring | `common/config.rs`, `common/state.rs`, `domains.rs`, `app.rs`: optional private R2 configuration, one shared client/service, authenticated routes and OpenAPI registration |
| Backend media | `domains/media/{config,policy,storage,validation,service,api,openapi}.rs`: policy, AWS signing/storage adapter, bounded actual WebP decode, ownership, completion, reconciliation, cleanup and contracts |
| Existing content | `domains/content/{dto,repository,seo}.rs`: optional `assetIds`, atomic reference validation/save, reusable record projection, and unchanged public sitemap behavior |
| Runtime | `main.rs`, `common/error.rs`, `Cargo.toml`, `Cargo.lock`, `Dockerfile`: cleanup command/scheduler, safe errors, pinned SDK/decoder dependencies and native build prerequisites |
| Backend checks | `domains/media/tests.rs`, `tests/portfolio_media.rs`, existing test config constructors: policy/codec/signing and PostgreSQL-backed lifecycle/security/cleanup tests |
| Storage rollout | `.env.portfolio.example`, `deploy/r2-staging-cors.json`, `examples/smoke_r2_upload.rs`: placeholders, actual signed-header CORS policy and opt-in real-account smoke test |
| Frontend core | `src/lib/media/{image-presets,convert-to-webp,direct-upload}.ts`: shared purposes, client-only conversion and completion-preserving direct upload/retry state |
| Frontend integration | `src/queries/assets/{upload,useProjectGalleryUpload}.ts`, `ImageUploadContext.tsx`: one reusable hook, whole-form pending state and verified IDs |
| Admin UI | `ContentEditor.tsx`, `ProjectGalleryField.tsx`, `RichTextEditor.tsx`, `ImageUploadStatus.tsx`, `ServiceFields.tsx`: all image entry points, previews, crop notices, status/retry/cancel and save gating |
| Private JSON bridge | `src/server/direct-image-uploads.ts`, `src/routes/api/admin/media.ts`, `src/server/media.ts`, `src/server/portfolio-content.ts`: same-origin metadata forwarding, strict action/ID/size limits, feature flag and image-only legacy cutover |
| Frontend checks | `tests/direct-image-upload.test.ts`, `tests/image-upload-bridge.test.ts`, gallery/CMS mocks, `tests/browser/` and `.github/workflows/deploy.yml`: mocked request-path tests, Origin/rollout checks and actual browser canvas tests in CI |

All frontend paths are relative to `adnan-portfolio-frontend`; backend domain paths are under `src/portfolio`. The frontend `.env.example` and `package.json` add only a rollout placeholder and the opt-in browser check command. No real environment files or credentials are modified.

## Purpose policy

| Purposes | Output | Maximum |
| --- | --- | --- |
| `blog_cover`, `project_cover`, `case_study_cover` | Centered 16:9 crop, no stretching or upscaling | 1920×1080, 2 MiB |
| `blog_inline`, `project_inline`, `case_study_inline` | Original ratio, no upscaling | 1600 px long side, 2 MiB |
| `project_gallery`, `case_study_gallery`, `service_image` | Original ratio, no upscaling | 1600 px long side, 2 MiB |

Source maximum is 20 MiB and 25 megapixels. JPEG/PNG/WebP signatures and animation markers are checked before browser decoding. PNG alpha is retained, EXIF orientation is applied by `createImageBitmap`, and canvas output is required to have actual WebP MIME/signature. GIF, animated WebP/PNG, SVG, AVIF, and HEIC are explicitly rejected in this workflow. Existing images in those formats are not altered.

Covers show the converted centered crop before publication. Galleries, rich-text images and service media are not cropped. Qualities tried are 0.82, 0.75, and 0.68. No original-byte fallback occurs after conversion failure. Upload UI shows stages, converted dimensions/sizes, errors, retry and cancellation; Save stays disabled while any image upload is active, including rich-text uploads.

## Private Rust API

All endpoints use the existing `/api/v1/admin` bearer authentication. The browser cannot call the private Axum host directly; the TanStack JSON bridge applies same-origin cookie/CSRF checks and only forwards an allowlisted action.

- `GET /api/v1/admin/uploads/policy` returns `{ "enabled": true }` only when Rust R2 configuration is enabled. An image-write permission is required.
- `POST /api/v1/admin/uploads/presign`: `{ purpose, originalName, contentType: "image/webp", sizeBytes, width, height }`; returns `201` with `{ uploadId, method: "PUT", uploadUrl, headers, expiresAt }`.
- `POST /api/v1/admin/uploads/{id}/complete`: returns a ready `{ assetId, status, publicUrl, key, contentType, sizeBytes, width, height, purpose }`.
- `DELETE /api/v1/admin/uploads/{id}` abandons an owned pending session.
- Existing content PUT/POST accepts optional `assetIds: UUID[]`; URLs remain compatibility/display fields, not authority for a new managed asset.

Keys and IDs are allocated by Rust, never derived from filenames. Signing is limited to 30 requests per actor per minute and 20 unexpired pending sessions (maximum 40 MiB declared). URLs expire after 300 seconds by default; configured TTL must be 30–300 seconds. Presigned headers, including `Content-Type` and `If-None-Match: *`, are forwarded exactly; Host is supplied by the browser from the URL. The pinned AWS SDK uses checksum calculation only when required by the operation, avoiding implicit SDK checksum headers for an unsigned payload. Review the smoke test and CORS when upgrading it.

Never log signed URLs, full rendered Compose JSON, R2 keys, or authorization values. Errors use the existing API envelope and sanitize SDK failures. CORS is not authentication or a size limit; the backend checks actual bytes and dimensions.

## One-time R2 and environment setup

1. Create a genuinely private staging bucket and a separate public media bucket. A public bucket's `pending/` prefix is not private.
2. Attach a production custom domain to the public bucket. Do not use that CDN domain for presigned PUTs.
3. Create bucket-scoped Object Read & Write credentials for these two buckets. Put the new image credentials only in the Rust environment (`.env.portfolio`); never in browser/Vite variables.
4. Set the placeholder entries from `.env.portfolio.example` with real names/domain and `R2_DIRECT_UPLOAD_ENABLED=true` only after provisioning. Keep the existing database settings unchanged.
5. Apply `deploy/r2-staging-cors.json` in the private staging bucket's dashboard CORS editor. Its origins are localhost 3000/5173 and `https://adnanhk.dev`; adjust to actual admin origins, without wildcards. Inspect actual presign response header **names**, not the signed URL, when checking CORS.
6. Set an optional 2-day private staging lifecycle expiry as a fallback. Do not attach a public domain to staging. Published objects use `image/webp` and `public, max-age=31536000, immutable`.

The frontend SSR environment uses `ADMIN_DIRECT_R2_UPLOAD=true` to enable this flow; its default is false. This is a runtime private environment variable, not `VITE_*`. Set it on the Ubuntu systemd service's existing `.env.production` and restart/redeploy the frontend. When false, legacy image rollback remains explicit: the browser still converts to WebP, then uploads the converted file through the old server route. Direct-flow errors never silently trigger this fallback. Existing service videos retain their separate legacy route and server-side storage credentials; they are not moved into this image API.

## Consistency and cleanup

Completion holds the upload row lock while validating and publishing, so duplicate completions return the same ready asset. The immutable public key/asset ID is allocated at presign time. If copy succeeds but the database transaction fails, retry finds the final object with matching upload metadata, validates it, and completes the database without making another object. A failed copy never creates a ready row. Expired sessions cannot publish a new object, but an already-copied object can be reconciled after expiry.

Rust `serve` runs cleanup every 15 minutes when R2 is enabled. For manual operations the container also supports `portfolio-api cleanup-media`. Cleanup waits at least 10 minutes beyond signature expiry before deleting staged objects, so write-once staging cannot be recreated with a still-valid URL. Deletion failures leave durable rows for retry. Completed staging objects and abandoned/failed/expired sessions are reclaimed; copied objects with failed DB commits are reconciled first. Unreferenced ready assets have a 7-day grace period; deletion first commits a `deleting` state to prevent an entity attaching a now-missing object. Any relational reference protects an asset, including references from soft-deleted content. Old legacy URL-only objects are deliberately not automatically reclaimed.

## Rollout and verification checklist

1. Keep both flags false. Back up the production database; publish the backend and apply its additive migration through the existing deployment workflow. Database and volume identities stay unchanged.
2. Configure the buckets, scoped backend credentials, CDN and staging CORS; enable the backend flag and deploy. Existing content still renders with legacy URLs.
3. Smoke-test R2 in a nonproduction deployment using the opt-in example described below. Do not enable the frontend flag until conditional PUT, copy, MIME/cache headers, and public reads work on the real account.
4. Enable the frontend flag on a staging/admin cohort, then test JPEG, transparent PNG, portrait cover, square gallery, and existing WebP on Chrome, Firefox, Edge and Safari. Check EXIF orientation and transparency visually.
5. DevTools Network must show only small JSON POSTs to `/api/admin/media` and the large raw `PUT` to `<account>.r2.cloudflarestorage.com`. No large image POST may go to localhost/Axum/the SSR server. Do not export an unsanitized HAR containing signatures or cookies.
6. Save and reopen blog, project, case-study and service forms. Verify new `assetIds`, old media previews, rich-text images, replacements, and reused asset retention. Check cancel, offline, failed PUT, expired signatures, duplicate complete and retry-after-complete-failure.
7. Monitor conversion errors, HTTP 403/412, completion lag and cleanup failures. Roll back by disabling the frontend flag; keep the new tables, old URL fields and published assets. If disabling the backend flag too, retain configuration until queued cleanup has finished or schedule cleanup under the enabled configuration separately.

Local checks: `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets`, `cargo test --locked --all-targets`. Disposable PostgreSQL integration: `DATABASE_URL=... cargo test --locked --all-targets -- --include-ignored`. Tests use memory-backed storage and do not contact Cloudflare. The actual SDK signing test signs locally using fake credentials. Browser encoding/network and real R2 compatibility are separate required acceptance checks, not implied by these tests passing.

## Opt-in real R2 smoke test

With a test deployment configured for dedicated R2 buckets and a short-lived authorized test account token, set `R2_SMOKE_ACCESS_TOKEN` in the process environment, then run `R2_SMOKE_TEST=true cargo run --locked --example smoke_r2_upload`. The example uses only the private API at `http://127.0.0.1:8080` by default (`R2_SMOKE_API_URL` can specify another loopback port). It signs a tiny valid WebP, performs a raw direct PUT, tests replay rejection with 412, completes twice, and fetches the public object to verify bytes, MIME and cache headers. It creates one unreferenced public test asset and staging object; normal cleanup reclaims them after their grace periods. Never run it against real data without deliberately opting in, and never put a token in shell history or a committed file.

## Workspace verification, 2026-10-25

- Frontend TypeScript and production SSR build passed; 156 Bun tests passed.
- Actual headless Chrome canvas conversion checks passed, including three consecutive runs of the final browser harness. The check is also wired into frontend CI; the GitHub run itself has not been executed here.
- Rust formatting and Clippy passed. All 50 Rust tests passed with `--include-ignored`, including 13 PostgreSQL-backed integration tests, on a separate temporary loopback PostgreSQL 18 cluster. CI retains PostgreSQL 17. The existing local database and production database were not used.
- Real codec tests cover VP8, VP8L, alpha VP8X, malformed/truncated images, animation rejection and dimension mismatches. The SDK signs locally with fake keys; lifecycle tests use memory-backed R2 storage.
- Both workflows passed actionlint; public JS was checked for accidental server-only R2 credential identifiers.
- Real Cloudflare PUT/412/HEAD/cross-bucket COPY/public GET/CORS and interactive admin DevTools acceptance were not executed: dedicated bucket configuration and credentials were not supplied. Cross-browser/EXIF visual checks remain part of rollout. Do not treat local tests as real-account verification.
- No production migration/deployment or image publication was performed. Docker image building was not checked locally because the Docker engine is stopped. Both rollout flags remain disabled in the examples until the provisioning and smoke-test checklist is satisfied.

References: [R2 presigned URLs](https://developers.cloudflare.com/r2/api/s3/presigned-urls/), [R2 CORS](https://developers.cloudflare.com/r2/buckets/cors/), [S3 compatibility](https://developers.cloudflare.com/r2/api/s3/api/).
