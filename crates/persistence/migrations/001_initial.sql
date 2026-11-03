CREATE TABLE profiles (
 account_id INTEGER PRIMARY KEY CHECK(account_id > 0 AND account_id < 4294967295),
 steam64 TEXT NOT NULL,
 created_at INTEGER NOT NULL
);
CREATE TABLE matches (
 account_id INTEGER NOT NULL REFERENCES profiles(account_id),
 match_id TEXT NOT NULL,
 hero_id INTEGER NOT NULL CHECK(hero_id > 0),
 start_time INTEGER NOT NULL,
 duration INTEGER, game_mode INTEGER, patch INTEGER, player_slot INTEGER, radiant_win INTEGER,
 kills INTEGER, deaths INTEGER, assists INTEGER, last_hits INTEGER, denies INTEGER,
 gold_per_min INTEGER, xp_per_min INTEGER, lane_role INTEGER,
 lineup_complete INTEGER NOT NULL DEFAULT 0 CHECK(lineup_complete IN (0,1)),
 source TEXT NOT NULL,
 fetched_at INTEGER NOT NULL,
 PRIMARY KEY(account_id, match_id)
);
CREATE INDEX matches_match_id ON matches(match_id);
CREATE INDEX matches_account_time ON matches(account_id,start_time DESC);
CREATE INDEX matches_account_hero_time ON matches(account_id,hero_id,start_time DESC);
CREATE INDEX matches_patch ON matches(account_id,patch);
CREATE TABLE match_heroes (
 account_id INTEGER NOT NULL,
 match_id TEXT NOT NULL,
 side TEXT NOT NULL CHECK(side IN ('ally','enemy')),
 hero_id INTEGER NOT NULL CHECK(hero_id > 0),
 PRIMARY KEY(account_id,match_id,side,hero_id),
 FOREIGN KEY(account_id,match_id) REFERENCES matches(account_id,match_id) ON DELETE CASCADE
);
CREATE INDEX match_heroes_lookup ON match_heroes(hero_id,side);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE sync_jobs (
 account_id INTEGER PRIMARY KEY REFERENCES profiles(account_id),
 status TEXT NOT NULL,
 offset INTEGER NOT NULL DEFAULT 0,
 pages_fetched INTEGER NOT NULL DEFAULT 0,
 imported INTEGER NOT NULL DEFAULT 0,
 updated INTEGER NOT NULL DEFAULT 0,
 failed INTEGER NOT NULL DEFAULT 0,
 missing_lineups INTEGER NOT NULL DEFAULT 0,
 from_time INTEGER NOT NULL,
 to_time INTEGER NOT NULL,
 last_synced_at INTEGER,
 error TEXT
);
CREATE TABLE import_failures (
 account_id INTEGER NOT NULL REFERENCES profiles(account_id),
 record_key TEXT NOT NULL,
 reason TEXT NOT NULL,
 observed_at INTEGER NOT NULL,
 PRIMARY KEY(account_id,record_key)
);
