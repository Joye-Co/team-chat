-- Member characters: the jojo-friends character id (for example 'jojo' or 'o03') that a
-- member's avatar draws, or NULL for the initial avatar. Validated against the 26 known ids
-- at the API boundary (docs/protocol.md, Member.character).
ALTER TABLE members ADD COLUMN character TEXT;
