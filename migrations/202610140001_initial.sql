CREATE TABLE users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE CHECK (email = lower(email)),
    name TEXT NOT NULL,
    headline TEXT NOT NULL DEFAULT 'Contributor',
    password_hash TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE roles (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL,
    protected BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE TABLE permissions (name TEXT PRIMARY KEY, description TEXT NOT NULL);
CREATE TABLE user_roles (
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, role_id)
);
CREATE TABLE role_permissions (
    role_id UUID REFERENCES roles(id) ON DELETE CASCADE,
    permission TEXT REFERENCES permissions(name),
    PRIMARY KEY (role_id, permission)
);
CREATE TABLE auth_sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    refresh_hash TEXT NOT NULL UNIQUE,
    access_expires_at TIMESTAMPTZ NOT NULL,
    refresh_expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX auth_sessions_user_idx ON auth_sessions(user_id);
CREATE TABLE used_refresh_tokens (
    token_hash TEXT PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES auth_sessions(id) ON DELETE CASCADE
);
CREATE TABLE content_items (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('blog', 'project')),
    slug TEXT NOT NULL CHECK (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(slug) <= 120),
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published', 'archived')),
    fields JSONB NOT NULL CHECK (jsonb_typeof(fields) = 'object'),
    editor_content JSONB,
    rendered_html TEXT NOT NULL,
    plain_text TEXT NOT NULL,
    seo JSONB NOT NULL DEFAULT '{}',
    faqs JSONB NOT NULL DEFAULT '[]',
    author_id UUID NOT NULL REFERENCES users(id),
    published_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    search_document TSVECTOR GENERATED ALWAYS AS (to_tsvector('english'::regconfig, title || ' ' || description || ' ' || plain_text)) STORED,
    UNIQUE (kind, slug),
    CHECK (status <> 'published' OR published_at IS NOT NULL)
);
CREATE TABLE content_slugs (
    kind TEXT NOT NULL,
    slug TEXT NOT NULL,
    content_id UUID NOT NULL REFERENCES content_items(id),
    PRIMARY KEY (kind, slug)
);
CREATE INDEX content_public_idx ON content_items(kind, published_at DESC, id) WHERE status = 'published' AND deleted_at IS NULL;
CREATE INDEX content_search_idx ON content_items USING GIN(search_document);
CREATE INDEX content_category_idx ON content_items(kind, (fields->>'category'));

INSERT INTO permissions(name, description) VALUES
('users:read', 'Read user accounts'), ('users:write', 'Provision and update users'),
('roles:read', 'Read roles and permissions'), ('roles:write', 'Manage roles and assignments'),
('content:read', 'Read drafts and archived content'),
('blogs:write', 'Create and edit blog drafts'), ('blogs:publish', 'Publish, archive, or edit live blogs'), ('blogs:delete', 'Delete blogs'),
('projects:write', 'Create and edit project drafts'), ('projects:publish', 'Publish, archive, or edit live projects'), ('projects:delete', 'Delete projects');
INSERT INTO roles(id, name, description, protected) VALUES
('00000000-0000-0000-0000-000000000001', 'owner', 'Bootstrap owner with all permissions', TRUE),
('00000000-0000-0000-0000-000000000002', 'admin', 'Content administration and publishing', TRUE),
('00000000-0000-0000-0000-000000000003', 'editor', 'Draft creation and editing', TRUE),
('00000000-0000-0000-0000-000000000004', 'viewer', 'Read-only administration access', TRUE);
INSERT INTO role_permissions SELECT '00000000-0000-0000-0000-000000000001'::uuid, name FROM permissions;
INSERT INTO role_permissions SELECT '00000000-0000-0000-0000-000000000002'::uuid, name FROM permissions WHERE name NOT IN ('users:write', 'roles:write');
INSERT INTO role_permissions SELECT '00000000-0000-0000-0000-000000000003'::uuid, name FROM permissions WHERE name IN ('content:read', 'blogs:write', 'projects:write');
INSERT INTO role_permissions VALUES ('00000000-0000-0000-0000-000000000004', 'content:read');
