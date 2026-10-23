ALTER TABLE content_items DROP CONSTRAINT content_items_kind_check;
ALTER TABLE content_items ADD CONSTRAINT content_items_kind_check CHECK (kind IN ('blog', 'project', 'case_study'));

INSERT INTO permissions(name, description) VALUES
('case-studies:write', 'Create and edit case study drafts'),
('case-studies:publish', 'Publish, archive, or edit live case studies'),
('case-studies:delete', 'Delete case studies');

INSERT INTO role_permissions(role_id, permission)
SELECT roles.id, permissions.name FROM roles CROSS JOIN permissions
WHERE roles.name IN ('owner', 'admin') AND permissions.name LIKE 'case-studies:%';

INSERT INTO role_permissions(role_id, permission)
SELECT id, 'case-studies:write' FROM roles WHERE name = 'editor';

CREATE INDEX content_case_industry_idx ON content_items((fields->>'industry')) WHERE kind = 'case_study' AND deleted_at IS NULL;
