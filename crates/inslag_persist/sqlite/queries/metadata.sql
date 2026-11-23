-- name: SlotHeader :one
SELECT generation, created_at_ms FROM meta WHERE singleton = 1;

-- name: AdvanceGeneration :exec
UPDATE meta SET generation = sqlc.arg(next_generation),
    created_at_ms = CASE WHEN generation = 0
        THEN CAST(round(unixepoch('subsec') * 1000) AS INTEGER)
        ELSE created_at_ms END
WHERE singleton = 1;
