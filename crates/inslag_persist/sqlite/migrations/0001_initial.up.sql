CREATE TABLE meta (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    generation INTEGER NOT NULL CHECK(generation >= 0),
    created_at_ms INTEGER NOT NULL CHECK(created_at_ms >= 0)
) STRICT;
INSERT INTO meta VALUES (1, 0, 0);

CREATE TABLE types (
    key TEXT PRIMARY KEY,
    kind INTEGER NOT NULL CHECK(kind IN (0, 1))
) STRICT;

CREATE TABLE entities (id INTEGER PRIMARY KEY CHECK(id >= 0)) STRICT;
INSERT INTO entities VALUES (0);

CREATE TABLE records (
    entity INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    type_key TEXT NOT NULL REFERENCES types(key),
    payload BLOB NOT NULL,
    PRIMARY KEY(entity, type_key)
) STRICT;

CREATE TABLE entity_refs (
    source_entity INTEGER NOT NULL,
    source_type TEXT NOT NULL,
    target INTEGER NOT NULL REFERENCES entities(id) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY(source_entity, source_type) REFERENCES records(entity, type_key) ON DELETE CASCADE,
    PRIMARY KEY(source_entity, source_type, target)
) STRICT;
CREATE INDEX entity_refs_target ON entity_refs(target);
