CREATE TABLE shader_activations (
    shader_id TEXT NOT NULL CHECK (shader_id ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    version INTEGER NOT NULL CHECK (version > 0),
    contract TEXT NOT NULL CHECK (contract ~ '^[a-f0-9]{64}$'),
    active BOOLEAN NOT NULL,
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (shader_id, version, contract)
);
