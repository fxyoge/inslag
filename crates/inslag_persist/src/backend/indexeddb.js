const VERSION = 1;
const MAX_GENERATION = (1n << 64n) - 1n;
const STORES = ["saves", "entities", "records"];

function saveError(name, message, properties) {
    return Object.assign(new Error(message), { name }, properties);
}

function openDatabase(name) {
    return new Promise((resolve, reject) => {
        const request = indexedDB.open(name, VERSION);
        let blocked = false;
        request.onblocked = () => {
            blocked = true;
            reject(new DOMException("Save database upgrade is blocked by another tab", "InvalidStateError"));
        };
        request.onupgradeneeded = () => {
            if (blocked) {
                request.transaction.abort();
                return;
            }
            const db = request.result;
            db.createObjectStore("saves").createIndex("collection", "collection");
            db.createObjectStore("entities");
            db.createObjectStore("records").createIndex("target", "targets", { multiEntry: true });
        };
        request.onerror = () => reject(request.error);
        request.onsuccess = () => {
            const db = request.result;
            if (blocked) {
                db.close();
                return;
            }
            db.onversionchange = () => db.close();
            resolve(db);
        };
    });
}

async function transaction(database, stores, mode, work) {
    const db = await openDatabase(database);
    try {
        if (!STORES.every((name) => db.objectStoreNames.contains(name))) {
            throw saveError("SaveFormat", "Not an Inslag save database");
        }
        return await new Promise((resolve, reject) => {
            const tx = mode === "readwrite"
                ? db.transaction(stores, mode, { durability: "strict" })
                : db.transaction(stores, mode);
            let failure, value;
            tx.oncomplete = () => resolve(value);
            tx.onabort = () => reject(failure ?? tx.error ?? new DOMException("Save transaction aborted", "AbortError"));
            const fail = (error) => {
                if (failure === undefined) {
                    failure = error;
                    tx.abort();
                }
            };
            const check = (request, validate) => {
                request.onsuccess = () => {
                    if (failure !== undefined) return;
                    try {
                        validate(request.result);
                    } catch (error) {
                        fail(error);
                    }
                };
            };
            const operation = work(tx, check);
            // Queue dependent requests while the transaction is active in the success event.
            const next = (input) => {
                const step = operation.next(input);
                if (step.done) value = step.value;
                else check(step.value, next);
            };
            try {
                next(undefined);
            } catch (error) {
                fail(error);
            }
        });
    } finally {
        db.close();
    }
}

function prefixed(key) {
    // All remaining key parts are strings, which sort before an array key part.
    return IDBKeyRange.bound(key, [...key, []], false, true);
}

function header(entry) {
    if (
        typeof entry?.generation !== "string" ||
        !/^[1-9][0-9]{0,19}$/.test(entry.generation) ||
        BigInt(entry.generation) > MAX_GENERATION ||
        !Number.isSafeInteger(entry.createdAt) ||
        entry.createdAt < 0 ||
        !Array.isArray(entry.manifest)
    ) {
        throw saveError("SaveInvalidData", "Invalid save metadata");
    }
    return entry;
}

export async function list_saves(database, collection) {
    return transaction(database, ["saves"], "readonly", function* (tx) {
        const entries = yield tx.objectStore("saves").index("collection").getAll(collection);
        return JSON.stringify(entries.map((entry) => [entry.handle, header(entry).createdAt]));
    });
}

export async function read_save(database, collection, handle) {
    return transaction(database, STORES, "readonly", function* (tx) {
        const slot = [collection, handle];
        const entry = yield tx.objectStore("saves").get(slot);
        if (entry === undefined) throw saveError("SaveNotFound", "Save does not exist");
        const { generation, manifest } = header(entry);
        const range = prefixed(slot);
        const entities = (yield tx.objectStore("entities").getAllKeys(range)).map((key) => key[2]);
        const records = yield tx.objectStore("records").getAll(range);
        return [
            JSON.stringify({
                generation,
                manifest,
                entities,
                records: records.map(({ owner, key }) => [owner, key]),
            }),
            records.map(({ payload }) => payload),
        ];
    });
}

export async function commit_save(database, collection, handle, metadata, payloads) {
    const change = JSON.parse(metadata);
    return transaction(database, STORES, "readwrite", function* (tx, check) {
        const saves = tx.objectStore("saves");
        const entities = tx.objectStore("entities");
        const records = tx.objectStore("records");
        const slot = [collection, handle];
        const entry = yield saves.get(slot);
        const previous = entry === undefined ? "0" : header(entry).generation;
        if (previous !== change.expected) {
            throw saveError("SaveConflict", "Save generation changed", {
                expected: change.expected, actual: previous,
            });
        }
        if (BigInt(previous) === MAX_GENERATION) {
            throw saveError("SaveInvalidData", "Save generation exhausted");
        }
        if (entry !== undefined && !change.rewrite &&
            JSON.stringify(entry.manifest) !== JSON.stringify(change.manifest)) {
            throw saveError("SaveManifest", "Save manifest does not match");
        }
        if (change.replace || change.rewrite) {
            records.delete(prefixed(slot));
            entities.delete(prefixed(slot));
        }
        for (const id of change.added) entities.put(true, [...slot, id]);
        for (const [owner, key] of change.deletes) records.delete([...slot, owner, key]);

        const owners = new Set();
        const targets = new Set();
        const removed = new Set(change.removed);
        for (const [index, [owner, key, references]] of change.upserts.entries()) {
            if (owner !== "0") owners.add(owner);
            if (!removed.has(owner)) {
                for (const id of references) targets.add(id);
            }
            records.put({
                owner,
                key,
                payload: payloads[index],
                targets: references.map((id) => [...slot, id]),
            }, [...slot, owner, key]);
        }
        const requireEntity = (id) => check(entities.getKey([...slot, id]), (found) => {
            if (found === undefined) throw saveError("SaveInvalidData", "Record references a missing entity");
        });
        for (const id of owners) requireEntity(id);
        for (const id of removed) {
            records.delete(prefixed([...slot, id]));
            entities.delete([...slot, id]);
        }
        for (const id of targets) {
            if (!owners.has(id) || removed.has(id)) requireEntity(id);
        }
        for (const id of removed) {
            check(records.index("target").getKey([...slot, id]), (found) => {
                if (found !== undefined) {
                    throw saveError("SaveInvalidData", "Cannot delete a referenced entity");
                }
            });
        }
        saves.put({
            collection,
            handle,
            generation: (BigInt(previous) + 1n).toString(),
            createdAt: entry?.createdAt ?? Date.now(),
            manifest: change.manifest,
        }, slot);
    });
}
