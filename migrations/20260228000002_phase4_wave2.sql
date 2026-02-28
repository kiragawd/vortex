-- Add teams table
CREATE TABLE IF NOT EXISTS teams (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    max_concurrent_tasks INTEGER DEFAULT 100,
    max_dags INTEGER DEFAULT 50
);

-- Add team_id to users
ALTER TABLE users ADD COLUMN team_id TEXT REFERENCES teams(id);

-- Add team_id to dags
ALTER TABLE dags ADD COLUMN team_id TEXT REFERENCES teams(id);
