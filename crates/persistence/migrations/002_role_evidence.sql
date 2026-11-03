CREATE TABLE role_evidence (
 account_id INTEGER NOT NULL,
 match_id TEXT NOT NULL,
 evidence_json TEXT NOT NULL,
 fetched_at INTEGER NOT NULL,
 PRIMARY KEY(account_id, match_id),
 FOREIGN KEY(account_id, match_id) REFERENCES matches(account_id, match_id) ON DELETE CASCADE
);
CREATE INDEX role_evidence_account_time ON role_evidence(account_id, fetched_at);
