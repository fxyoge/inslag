SELECT NOT EXISTS (SELECT 1 FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*');
