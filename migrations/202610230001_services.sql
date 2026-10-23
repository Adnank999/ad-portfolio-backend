ALTER TABLE content_items DROP CONSTRAINT content_items_kind_check;
ALTER TABLE content_items ADD CONSTRAINT content_items_kind_check CHECK (kind IN ('blog', 'project', 'case_study', 'service'));

INSERT INTO permissions(name, description) VALUES
('services:write', 'Create and edit service drafts and navigation'),
('services:publish', 'Publish, disable, or edit live services'),
('services:delete', 'Delete services');

INSERT INTO role_permissions(role_id, permission)
SELECT roles.id, permissions.name FROM roles CROSS JOIN permissions
WHERE roles.name IN ('owner', 'admin') AND permissions.name LIKE 'services:%';

INSERT INTO role_permissions(role_id, permission)
SELECT id, 'services:write' FROM roles WHERE name = 'editor';
