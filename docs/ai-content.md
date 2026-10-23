# AI content generation

## Integration contract

The existing CMS writes `ContentWrite` through `/api/v1/admin/blogs` and `/api/v1/admin/projects`. Core fields, `editorContent` (Tiptap JSON), `fields` (blog/project details), `seo`, and `faqs` are separate. Images are uploaded by the existing R2 endpoint. `ContentEditor` owns unsaved form state; Tiptap currently initializes from JSON or the backend's sanitized legacy HTML. Generation must update that state and explicitly synchronize the editor without invoking a CRUD mutation.

`POST /api/v1/admin/ai/generate` uses existing bearer authentication and additionally requires the built-in `owner` or `admin` role and the relevant content-write permission. Requests use camelCase, consistent with the existing API. `contentType` is `blog` or `project`; `mode` is `create`, `regenerate`, or `fill_missing`. `contentContext` is a tagged blog/project object. `targetFields` and `existingFields` use explicit CMS paths such as `title`, `editorContent`, `fields.tags`, and `seo.metaTitle`.

The response contains `contentType`, `generationId`, `generatedFields` (a typed, allowlisted path/value patch), `warnings`, and `skippedFields`. Rich text is returned as valid Tiptap JSON. The provider receives a constrained block schema, which Rust converts to supported editor nodes and validates with the existing renderer. Image references, publishing controls, external URLs, and index/follow flags cannot be generated. Known image URLs and canonical URLs can only be derived deterministically, and existing asset/URL values are preserved.

The endpoint never inserts or updates a content record. Preview selections, explicit replacement confirmation, empty-only application, and field-level stale-value checks run before the existing form is changed. Save Changes remains the only content persistence action.

## Provider configuration

Set these variables only in the Rust backend environment (normally `Backend/.env.portfolio`):

```dotenv
AI_PROVIDER=gemini
AI_API_KEY=your-google-ai-studio-key
AI_MODEL=gemini-3.5-flash-lite
AI_REQUEST_TIMEOUT_SECS=90
AI_MAX_OUTPUT_TOKENS=8192
AI_MAX_CONCURRENT_REQUESTS=2
AI_REQUESTS_PER_ADMIN_HOUR=20
```

The feature is disabled when `AI_API_KEY` is absent. Only Gemini is implemented. The model is configurable without changing application code. The provider endpoint is fixed to Google's HTTPS API; callers cannot choose a destination. Keys are sent in a header, never in the URL. No image URL or reference URL is fetched by generation.

Restart the API after changing environment variables or provider code. For local development, stop the existing process with Ctrl+C before running `cargo run --bin portfolio-api -- serve` from `Backend`.

Gemini's `generationConfig.responseFormat.text.mimeType` is an enum: JSON output uses `APPLICATION_JSON`, not the MIME string `application/json`. Provider rejections log only the model, HTTP status, and attempt number, never provider bodies or credentials. An HTTP 400 indicates a rejected request; 401/403 indicate credential or access problems; 429 indicates quota or rate limiting.

Request bodies, provider response bytes, context length, output tokens, execution time, concurrency, and per-admin requests are bounded. Limits apply per API process; multi-replica deployments should also enforce a shared gateway quota. Closing the modal discards obsolete results; an already accepted provider request can still consume tokens. Ordinary logs contain identifiers, elapsed time, and outcome codes, not prompts or generated bodies.

Model output needs editorial review: schema validation cannot prove the semantic truth of prose. Unsupported technologies, dates, URLs, and novel numeric claims are omitted or rejected; missing information is reported as a warning. Slug uniqueness is still enforced on the normal save route.

## Token usage and cost estimates

Successful generation responses include `usage`, captured from Gemini's `usageMetadata` independently of the generated content. Input tokens include cached input. Output tokens include candidate and thinking tokens; thinking tokens and cached input are also shown separately as subsets. Total tokens come from the provider. Missing or malformed metadata is returned as `usage: null`, never made up or reported as zero. Derived-only requests do not call Gemini and have no usage metadata.

`usage.estimatedCost` includes USD input, output, and total estimates and the per-million-token rates used. The default for the exact model `gemini-3.5-flash-lite` is the [standard paid-tier pricing](https://ai.google.dev/gemini-api/docs/pricing#gemini-3.5-flash-lite): $0.30 input, $2.50 output (including thinking), and $0.03 cached input per million tokens. Verified on 2026-10-16. Cached tokens are deducted from ordinary input before pricing. These are estimates, not billing records: free-tier allowances, discounts, taxes, caching storage, failed/cancelled requests, and other charges are not included. Additional unaccounted tokens or an unknown model produce a null estimate, but known token usage still appears.

To update rates or price a different model, set all three backend overrides together:

```dotenv
AI_INPUT_USD_PER_MILLION_TOKENS=0.30
AI_OUTPUT_USD_PER_MILLION_TOKENS=2.50
AI_CACHED_INPUT_USD_PER_MILLION_TOKENS=0.03
```

Rates must be finite and non-negative; use zero rates if estimating a free tier. Restart the backend after changes. Rates and estimates are calculated only in Rust; no credentials are exposed. The admin modal shows the latest request and cumulative totals for successful generations during that open modal session, including regeneration. This is not persisted spending history and does not include requests that failed, were cancelled, or returned no usage.
