CREATE TABLE image_uploads (
    id UUID PRIMARY KEY,
    requested_by UUID NOT NULL REFERENCES users(id),
    purpose TEXT NOT NULL,
    staging_key TEXT NOT NULL UNIQUE,
    public_key TEXT NOT NULL UNIQUE,
    asset_id UUID NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','completed','failed','abandoned')),
    expected_size_bytes BIGINT NOT NULL CHECK (expected_size_bytes > 0),
    expected_width INT NOT NULL CHECK (expected_width > 0),
    expected_height INT NOT NULL CHECK (expected_height > 0),
    original_name TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    staging_cleaned_at TIMESTAMPTZ,
    public_cleaned_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX image_uploads_actor_created_idx ON image_uploads(requested_by, created_at);
CREATE INDEX image_uploads_cleanup_idx ON image_uploads(expires_at) WHERE staging_cleaned_at IS NULL;

CREATE TABLE media_assets (
    id UUID PRIMARY KEY,
    created_by UUID NOT NULL REFERENCES users(id),
    upload_id UUID NOT NULL UNIQUE REFERENCES image_uploads(id),
    purpose TEXT NOT NULL,
    public_key TEXT NOT NULL UNIQUE,
    public_url TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL CHECK (content_type = 'image/webp'),
    size_bytes BIGINT NOT NULL CHECK (size_bytes > 0),
    width INT NOT NULL CHECK (width > 0),
    height INT NOT NULL CHECK (height > 0),
    status TEXT NOT NULL DEFAULT 'ready' CHECK (status IN ('ready','deleting','deleted')),
    unreferenced_at TIMESTAMPTZ DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX media_assets_cleanup_idx ON media_assets(unreferenced_at) WHERE status = 'ready';

CREATE TABLE content_media_assets (
    content_id UUID NOT NULL REFERENCES content_items(id) ON DELETE CASCADE,
    asset_id UUID NOT NULL REFERENCES media_assets(id),
    PRIMARY KEY (content_id, asset_id)
);
CREATE INDEX content_media_assets_asset_idx ON content_media_assets(asset_id);
