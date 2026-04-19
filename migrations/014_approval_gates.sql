-- T-020: Additional indexes for approval gates CLI
-- The approval_requests table already exists (migration 002).
-- Add supplementary indexes for agent-initiated mutation workflows.
CREATE INDEX IF NOT EXISTS idx_approval_actor_created ON approval_requests(requester, created_at);
CREATE INDEX IF NOT EXISTS idx_approval_resource ON approval_requests(resource_type, resource_id);
