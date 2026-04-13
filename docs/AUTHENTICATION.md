# Authentication, Authorization & Security

## Overview

Ryuo provides a layered security model covering authentication, authorization, secrets management, network controls, and execution sandboxing.

**Modules:** `src/auth.rs`, `src/rbac.rs`, `src/vault.rs`

---

## Authentication

### Local Authentication

Username/password authentication with bcrypt-hashed passwords stored in PostgreSQL.

- Passwords are hashed with bcrypt before storage
- Rate-limited login: **10 attempts per 60 seconds** per username — returns `429 Too Many Requests` on exceeded limit
- Default admin credentials: `admin` / `admin` (change immediately in production)

```bash
curl -X POST http://localhost:3000/api/login \
  -H "Content-Type: application/json" \
  -d '{"username": "admin", "password": "admin"}'
```

### OIDC Integration

OpenID Connect flow supporting Okta, Azure AD, and PingIdentity:

- **Discovery document** — Auto-fetches `.well-known/openid-configuration` for provider endpoints
- **Token exchange** — Authorization code → access token → ID token validation
- **Userinfo fetching** — Retrieves user profile from the OIDC provider's userinfo endpoint
- **Session management** — Sessions tracked in `user_sessions` table with token storage

**CLI configuration:**

```bash
ryuo-cli auth-provider create \
  --name "okta-prod" \
  --type oidc \
  --client-id "your-client-id" \
  --client-secret "your-client-secret" \
  --issuer-url "https://your-org.okta.com"
```

### SAML / LDAP

Configuration types and session management are defined. Provider implementations are pending.

```bash
# List configured auth providers
ryuo-cli auth-provider list

# Test provider connectivity
ryuo-cli auth-provider test --name "okta-prod"
```

---

## Authorization (RBAC)

### Roles

| Role | Permissions |
|------|------------|
| **Admin** | Full access — users, secrets, teams, audit logs, all DAGs |
| **Operator** | DAG management — trigger, pause, edit, upload. Cannot manage users, secrets, or audit logs |
| **Viewer** | Read-only — DAGs, tasks, runs, swarm status |

### Fine-Grained Permissions

Permissions use a `resource.action` format with wildcard scope matching:

| Permission | Description |
|-----------|-------------|
| `dag.*` | All DAG operations |
| `dag.read` | Read DAG definitions |
| `dag.trigger` | Trigger DAG runs |
| `secret.read` | Read secrets |
| `secret.write` | Create/update secrets |
| `user.manage` | Create/delete users |
| `audit.read` | View audit logs |

**CLI management:**

```bash
# List roles and permissions
ryuo-cli rbac list-roles
ryuo-cli rbac list-permissions

# Assign/revoke roles
ryuo-cli rbac assign --user "alice" --role "Operator"
ryuo-cli rbac revoke --user "alice" --role "Operator"

# View user roles
ryuo-cli rbac user-roles --user "alice"
```

### Team Isolation

Multi-tenant support with per-team resource partitioning:

- Non-admin users see only their team's DAGs
- Per-team resource quotas enforce usage limits
- Admin users have cross-team visibility

```bash
# Team management
ryuo-cli team create --name "data-engineering" --quota-max-dags 100
ryuo-cli team list
ryuo-cli team delete --name "data-engineering"
```

---

## API Token Scoping

Tokens provide programmatic access with fine-grained restrictions:

- **Bcrypt-hashed** verification stored in database
- **Action/resource scoping** — tokens restricted to specific operations
- **Wildcard matching** — `dag.*` grants all DAG operations
- **Automatic expiry** — configurable TTL per token

```bash
# Create scoped tokens
ryuo-cli token create --name "ci-deploy" --scopes "dag.trigger,dag.read"
ryuo-cli token list
```

---

## IP Allowlisting

CIDR-based network access control at the middleware level:

- Supports **IPv4** and **IPv6** subnets
- Applied before authentication — blocked IPs never reach auth handlers
- Configured via API or database

---

## Secrets Vault

AES-256-GCM encrypted secrets management:

- **Unique nonces** per secret — no nonce reuse
- **At-rest encryption** — secrets encrypted in PostgreSQL
- **Environment variable injection** — decrypted secrets passed to task processes
- Requires `RYUO_SECRET_KEY` environment variable (32-character key)

```bash
# Secret management
ryuo-cli secrets set DB_PASSWORD "s3cr3t"
ryuo-cli secrets get DB_PASSWORD
ryuo-cli secrets list
ryuo-cli secrets delete DB_PASSWORD
```

See [Secrets Vault](./SECRETS_VAULT.md) for detailed encryption internals.

---

## Security Headers & Network

All HTTP responses include:

| Header | Value |
|--------|-------|
| `Content-Security-Policy` | Restrictive CSP |
| `X-Frame-Options` | `DENY` |
| `X-Content-Type-Options` | `nosniff` |

Additional protections:

- **Request body limits** — Bodies > 10 MB rejected with `413 Payload Too Large`
- **Path traversal protection** — DAG source updates validated against canonical `dags/` directory
- **TLS** — REST API supports TLS via `axum-server` + `rustls` (`--tls-cert`, `--tls-key` flags)
- **mTLS** — Planned for gRPC worker connections

---

## Execution Sandboxing

By default, Ryuo runs in a secure sandbox mode:

| Feature | Flag | Default |
|---------|------|---------|
| Python DAG execution | `--allow-unsafe-dag-exec` | **Disabled** |
| Dynamic plugin loading | `--allow-unsafe-plugins` | **Disabled** |

Both require explicit CLI opt-in. Without the flags, Python DAGs and `.so`/`.dylib` plugins are not loaded.

---

## Related Documentation

- [API Reference](./API_REFERENCE.md) — REST API endpoints and auth examples
- [Secrets Vault](./SECRETS_VAULT.md) — Encryption internals
- [Compliance](./COMPLIANCE.md) — Audit logging and governance
- [Deployment](./DEPLOYMENT.md) — TLS configuration and environment variables
