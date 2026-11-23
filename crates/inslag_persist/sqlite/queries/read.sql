-- name: ReadManifest :many
SELECT key, kind FROM types;

-- name: ReadEntities :many
SELECT id FROM entities WHERE id != 0 ORDER BY id;

-- name: ReadRecords :many
SELECT entity, type_key, payload FROM records;
