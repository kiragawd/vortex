-- SEC-8: Add password_change_required flag for forced password rotation.
-- Default admin credentials must require a password change on first login.

ALTER TABLE users ADD COLUMN IF NOT EXISTS password_change_required BOOLEAN NOT NULL DEFAULT FALSE;

-- Force password change for the default admin account.
UPDATE users SET password_change_required = TRUE WHERE username = 'admin';
