-- Workspace MCP servers. Agents share these tools.

CREATE TABLE IF NOT EXISTS mcp_servers (
    id TEXT PRIMARY KEY,
    space_id TEXT NOT NULL REFERENCES spaces (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    transport TEXT NOT NULL,
    command TEXT,
    args JSONB NOT NULL DEFAULT '[]'::jsonb,
    env JSONB NOT NULL DEFAULT '{}'::jsonb,
    url TEXT,
    headers JSONB NOT NULL DEFAULT '{}'::jsonb,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (space_id, user_id, name)
);

CREATE INDEX IF NOT EXISTS mcp_servers_space_user_idx ON mcp_servers (space_id, user_id, created_at);
