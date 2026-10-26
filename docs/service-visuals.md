# Service image and shader visuals

The optional `fields.visuals` map adds independent `hero`, `overview`, `spotlight`, `article-1`, `article-2`, and `article-3` choices to the existing service CRUD. Existing `fields.media` images/videos remain valid. Visuals live in the existing `content_items.fields` JSONB column; no content backfill is needed. The admin activation feature adds `migrations/202610270001_shader_activation.sql` and must be migrated before starting the updated backend. A selected visual takes precedence over old media for the same slot.

```json
{
  "type": "shader",
  "shader": "gradient-flow",
  "version": 1,
  "props": {
    "colorStops": ["#160009", "#C21E56", "#7C0A02"],
    "speed": 0.5,
    "amplitude": 1,
    "blend": 0.5,
    "direction": "horizontal"
  },
  "poster": "https://media.example.com/services/verified-asset.webp",
  "alt": "Crimson flowing gradient"
}
```

Image alternatives are `{ "type": "image", "src": "...", "alt": "..." }`. The outer map uses only the six slot names. Shader `poster` is optional legacy/SEO metadata; it may be omitted or empty. The frontend renders shaders over the page background, not a fallback image. A shader without a poster needs no upload or media asset reference. Existing saved posters remain accepted and protected; any nonempty new image/poster URL still requires a ready owned/permitted `service_image` asset referenced by `assetIds`. Image visuals continue to require a valid `src`.

`GET /api/v1/admin/shaders` uses existing authenticated middleware and requires `services:write`. It returns configured metadata/schema contracts, an `active` flag per entry, and `canManage` for publishers, never executable source. `PUT /api/v1/admin/shaders/{id}/activation` requires `services:publish`, accepts exactly `{version, contract, active}`, and returns the updated catalog. Unknown IDs are 404; stale versions/hashes are 409; unconfigured drafts cannot be activated. The frontend mutation retains the established same-origin and authenticated API bridge.

Activation is persisted in `shader_activations` by ID/version/contract with the actor and update timestamp. Valid configured source drafts default inactive; source-ready legacy contracts default active. Persisted overrides survive deployment; changed contracts do not inherit a different contract's override. Service writes require an active shader when assigning a new slot or replacing its shader/version. Existing assignments stay renderable/editable after deactivation, so disabling a library choice does not break live pages. Public reads do not add catalog queries.

Service writes use the normal RBAC/version checks; visual types, URLs, descriptions, IDs/versions and props are validated before persistence. Unknown keys/IDs/versions, oversized props, invalid sliders/colors and unverified poster URLs are rejected with the existing HTTP 422 envelope; wrong-purpose/owner assets are forbidden. No shader source, imports, callbacks or arbitrary code can be submitted.

`assets/shaders.manifest.json` is a generated build asset, included in Docker builds and embedded in the binary. Schema validators are compiled once and validated at application startup; remote HTTP/file schema resolution is disabled. Do not edit the backend artifact independently. In the sibling frontend run `bun run shader:sync` after reviewing definition changes, then commit both repositories. Backend unit tests validate every manifest default and reject invalid catalogs/props; PostgreSQL lifecycle tests cover persisted visuals, authentication and ready media references.

The Docker context permits only this manifest from `assets/`; other source bundles under that folder remain excluded. Hero visual images/posters also become the default social-preview image, unless an explicit SEO image override is saved.

The manifest now includes 21 configured effects: the original three plus 18 installed shaders with strict schemas and bounded controls. New contracts default inactive, ready for activation in Admin → Shaders after both deployments. Rebuild and restart the API to load the changed embedded manifest; rebuilding the frontend or copying JSON beside an old executable is not enough. No additional database migration is introduced for these new shader definitions. Activation still uses the existing migrated `shader_activations` table and publisher permission. Backend-first deployment avoids frontend/backend contract mismatches; refresh the admin catalog after deployment.

Deploy the migrated backend with the matching manifest before the frontend. Locally run `cargo run --locked --bin portfolio-api -- migrate`, then restart the API. Open **Admin → Shaders** (`/admin/shaders`) in the sidebar and activate a configured draft; no service form, further build or restart is needed for availability changes. Service editors retain section selection and prop controls, not global activation. The editor compares contract hashes against the API; mismatched effects cannot be selected/saved. Existing content/images remain readable. Do not remove or change a shader version until saved configurations are migrated. The optional-poster change itself requires no database migration, but deploy its backend support before saving poster-free shaders. New frontends use a plain page-colored shader fallback; older frontends may still use existing saved posters.

## Catalog errors and migration recovery

The activation feature requires `202610270001_shader_activation.sql`. `serve` does not automatically migrate the database. If the activation table is missing, catalog reads, activation and new shader assignments fail closed with HTTP 503, code `shader_schema_unavailable`, and an actionable message. The backend logs the database error and the need to apply migrations. Other database errors retain the existing error handling rather than being misreported as a missing migration. Public service reads do not depend on the activation table.

For local development, stop the old API if rebuilding would lock its executable, then run from `Backend` with the correct backend environment:

```bash
cargo run --locked --bin portfolio-api -- migrate
cargo run --locked --bin portfolio-api -- serve
```

Use the same `DATABASE_URL` as the running API; process environment variables can override `.env`. If migration reports an authentication failure, correct the database connection first. Do not share credentials or bypass the migration by creating tables manually. To inspect the schema safely in a database console connected to that database, run `SELECT to_regclass('shader_activations');`. A null result means the table is not visible on that connection's search path.

For production, deploy the updated backend image through the existing CI/CD pipeline. `deploy/update-api.sh` runs `api migrate` with the production Compose environment before recreating only the API. Ensure the new migration is committed and included in the image; pulling only old source or restarting an older image does not add it. Never reset the database or remove PostgreSQL volumes to fix this error. Once migration succeeds, open Admin → Shaders and Retry shader catalog.

The HTTP trace's generic 500 line alone does not establish the cause. Inspect the adjacent `Database operation failed` log for connection, permissions or schema errors; the new missing-schema diagnostic specifically identifies migration recovery.

The full authoring, installation, cleanup and release instructions are in the frontend `src/components/shaders/README.md`. The website never runs shadcn/package-installation commands. `shader:add` is a trusted developer command only.
