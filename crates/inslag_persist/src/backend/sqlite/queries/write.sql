-- name: ClearRecords :exec
DELETE FROM records;

-- name: ClearEntities :exec
DELETE FROM entities WHERE id != 0;

-- name: ClearManifest :exec
DELETE FROM types;

-- name: InsertType :exec
INSERT INTO types (key, kind) VALUES (sqlc.arg(key), sqlc.arg(kind));

-- name: InsertEntity :exec
INSERT OR IGNORE INTO entities (id) VALUES (sqlc.arg(id));

-- name: DeleteRecord :exec
DELETE FROM records WHERE entity = sqlc.arg(entity) AND type_key = sqlc.arg(type_key);

-- name: UpsertRecord :exec
INSERT INTO records (entity, type_key, payload)
VALUES (sqlc.arg(entity), sqlc.arg(type_key), sqlc.arg(payload))
ON CONFLICT(entity, type_key) DO UPDATE SET payload = excluded.payload;

-- name: ClearReferences :exec
DELETE FROM entity_refs
WHERE source_entity = sqlc.arg(source_entity) AND source_type = sqlc.arg(source_type);

-- name: InsertReference :exec
INSERT INTO entity_refs (source_entity, source_type, target)
VALUES (sqlc.arg(source_entity), sqlc.arg(source_type), sqlc.arg(target));

-- name: DeleteEntity :exec
DELETE FROM entities WHERE id = sqlc.arg(id);
