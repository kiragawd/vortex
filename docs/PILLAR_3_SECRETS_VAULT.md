# Pillar 3: Secrets Vault — Encrypted Secret Management

## Overview

The VORTEX Secrets Vault provides encrypted storage for sensitive data (database credentials, API keys, tokens, etc.) within distributed DAG workflows. Secrets are encrypted at rest using AES-256-GCM with unique nonces, ensuring that even if the database is compromised, secret values remain protected.

### Key Security Properties

| Property | Implementation |
|----------|----------------|
| **Encryption Algorithm** | AES-256-GCM (Authenticated Encryption with Associated Data) |
| **Nonce Size** | 96 bits (12 bytes), randomly generated per encryption |
| **Key Source** | `VORTEX_SECRET_KEY` environment variable (32-character string) |
| **Storage Format** | Base64-encoded (nonce + ciphertext) in a TEXT column |
| **Integrity** | GCM authentication tag ensures ciphertext hasn't been modified |
| **Freshness** | Unique nonce per secret prevents replay attacks |

---

## Schema & Storage

### Database Table Structure

The `secrets` table stores all encrypted secrets:

```sql
CREATE TABLE IF NOT EXISTS secrets (
    key        TEXT        PRIMARY KEY,
    value      TEXT        NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
```

| Column | Type | Description |
|--------|------|-------------|
| `key` | TEXT | Human-readable secret name (e.g., `DB_PASSWORD`, `SLACK_TOKEN`) |
| `value` | TEXT | Base64-encoded string containing nonce (12 bytes) + AES-256-GCM ciphertext |
| `updated_at` | TIMESTAMPTZ | Timestamp of last update |

### Encryption Mechanism

When a secret is stored:

1. **Generate a random 96-bit nonce** (12 bytes)
2. **Encrypt the plaintext value** using AES-256-GCM with:
   - Key: Raw bytes of `VORTEX_SECRET_KEY` (32 characters = 32 bytes = 256 bits)
   - Plaintext: User-provided secret value
   - Nonce: Randomly generated per operation
3. **Combine nonce + ciphertext** into a single byte array
4. **Base64-encode** the combined bytes and store as TEXT in the database

### Key Format

VORTEX uses the raw bytes of the `VORTEX_SECRET_KEY` string directly as the AES-256 key:

```rust
let key_str = env::var("VORTEX_SECRET_KEY")?;
let key_bytes = key_str.as_bytes();  // Raw bytes, NOT hex-decoded
assert_eq!(key_bytes.len(), 32);     // Must be exactly 32 characters
```

**Important:** The key must be exactly 32 characters (not 64 hex characters). Each character contributes one byte to the 256-bit key.

```bash
# Generate a valid key
export VORTEX_SECRET_KEY=$(head -c 32 /dev/urandom | LC_ALL=C tr -dc 'a-zA-Z0-9' | head -c 32)
```

---

## API Reference

### 1. List Secret Keys

**Endpoint:** `GET /api/secrets` — Admin only

Returns secret names only. Values are never exposed via the API.

```json
// Response (200)
{ "secrets": ["DB_PASSWORD", "SLACK_TOKEN", "API_KEY"] }
```

```bash
curl http://localhost:3000/api/secrets \
  -H "Authorization: Bearer <api_key>"
```

### 2. Store Secret

**Endpoint:** `POST /api/secrets` — Admin only

Creates or updates an encrypted secret.

```json
// Request
{ "key": "DB_PASSWORD", "value": "super_secret_123" }

// Response (200)
{ "message": "Secret stored successfully" }
```

```bash
curl -X POST http://localhost:3000/api/secrets \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <api_key>" \
  -d '{"key": "DB_PASSWORD", "value": "super_secret_123"}'
```

### 3. Delete Secret

**Endpoint:** `DELETE /api/secrets/:key` — Admin only

```bash
curl -X DELETE http://localhost:3000/api/secrets/DB_PASSWORD \
  -H "Authorization: Bearer <api_key>"
```

> **Note:** There is no API endpoint to retrieve (decrypt) a secret value. Secrets are only decrypted internally at task execution time and injected as environment variables.

---

## Task Secret Injection

### How Secrets Are Injected During Task Execution

When a task is dispatched to a worker:

1. The controller fetches all secrets from the vault
2. Secrets are decrypted using the `VORTEX_SECRET_KEY`
3. Decrypted values are injected as environment variables into the task process
4. The task can access secrets via `os.environ` (Python) or `$ENV_VAR` (Bash)

### Environment Variable Injection

Secrets are injected with their original key name as the environment variable:

```python
# In a PythonOperator task
import os
db_password = os.environ.get("DB_PASSWORD")
```

```bash
# In a BashOperator task
echo $DB_PASSWORD
```

### Additional Environment Variables

VORTEX also injects helper variables for tasks that need API access:

| Variable | Description |
|----------|-------------|
| `VORTEX_BASE_URL` | Base URL of the VORTEX server (default: `http://localhost:3000`) |
| `VORTEX_API_KEY` | Task-scoped API key (only if `VORTEX_TASK_API_KEY` is set on the server) |

> **Security Note:** Tasks do NOT receive the admin API key. If tasks need API access, set the `VORTEX_TASK_API_KEY` environment variable on the server process with a scoped, limited-privilege key.

---

## Security Best Practices

### 1. Protect the Master Key

- Store `VORTEX_SECRET_KEY` securely (e.g., HashiCorp Vault, AWS Secrets Manager, systemd credentials)
- Only the controller process should have access to the key
- Rotate after any suspected compromise

### 2. Use HTTPS

- Always use TLS in production when communicating with the Secrets API
- Configure with `--tls-cert` and `--tls-key` flags

### 3. Audit Secret Access

All secret operations are logged to the audit log:
- `secret.store` — When a secret is created or updated
- `secret.delete` — When a secret is deleted
- Secret names are logged; values are never logged

### 4. Never Log Secret Values

Secrets should never appear in logs, error messages, or stack traces. VORTEX enforces this at the API level by never returning decrypted values.

### 5. Use Scoped Task API Keys

If tasks need VORTEX API access, use `VORTEX_TASK_API_KEY` with a dedicated, limited-privilege API key rather than the admin key.

---

## Troubleshooting

### Error: "VORTEX_SECRET_KEY environment variable not set"

**Solution:** Set the environment variable before starting the server:
```bash
export VORTEX_SECRET_KEY=$(head -c 32 /dev/urandom | LC_ALL=C tr -dc 'a-zA-Z0-9' | head -c 32)
```

### Error: "VORTEX_SECRET_KEY must be exactly 32 bytes"

**Cause:** The key is not exactly 32 characters long.

**Solution:** Ensure the key is exactly 32 characters:
```bash
echo -n "$VORTEX_SECRET_KEY" | wc -c  # Should output 32
```

### Error: "Secret Vault is not initialized"

**Cause:** The server was started without `VORTEX_SECRET_KEY`. The vault is disabled (non-fatal).

**Solution:** Set the environment variable and restart the server.

### Error: "Decryption failure"

**Cause:** The ciphertext was encrypted with a different key than the one currently set.

**Solution:** Ensure `VORTEX_SECRET_KEY` matches the key used when the secret was stored.

---

## Related Documentation

- [API Reference](./API_REFERENCE.md) — Complete endpoint documentation
- [Deployment Guide](./DEPLOYMENT.md) — Setup and configuration
- [Resilience](./PILLAR_4_RESILIENCE.md) — Task recovery when workers fail
- [Architecture Overview](./ARCHITECTURE.md) — System design and data flow
