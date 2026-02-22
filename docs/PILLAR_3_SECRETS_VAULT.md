# Pillar 3: Secrets Vault — Encrypted Secret Management

## Overview

The VORTEX Secrets Vault is a secure, encrypted storage system for managing sensitive data (database credentials, API keys, tokens, etc.) within distributed DAG workflows. Every secret is encrypted at rest using AES-256-GCM with unique nonces, ensuring that even if the database is compromised, secrets remain protected.

### Why Encrypt Secrets at Rest?

In distributed systems, secrets are often stored in databases, configuration files, or environment variables. Without encryption, a database breach exposes all secrets immediately. VORTEX encrypts secrets at rest to:

- **Mitigate database breaches**: Encrypted values are useless without the encryption key
- **Comply with security standards**: Meet GDPR, HIPAA, and SOC 2 requirements
- **Protect against unauthorized access**: Only authorized processes with the encryption key can decrypt secrets
- **Enable safe auditing**: Secret names are visible in logs; values are not

### Key Security Properties

| Property | Implementation |
|----------|-----------------|
| **Encryption Algorithm** | AES-256-GCM (Authenticated Encryption with Associated Data) |
| **Nonce Size** | 96 bits (12 bytes), randomly generated per encryption |
| **Key Derivation** | VORTEX_SECRET_KEY environment variable (32 bytes for AES-256) |
| **Integrity** | GCM authentication tag ensures ciphertext hasn't been modified |
| **Freshness** | Unique nonce per secret prevents replay attacks |

---

## Schema & Storage

### Database Table Structure

The `secrets` table stores all encrypted secrets:

```sql
CREATE TABLE secrets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    encrypted_value BLOB NOT NULL,
    nonce BLOB NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT | Unique identifier (UUID format, e.g., `secret_abc123...`) |
| `name` | TEXT | Human-readable secret name (e.g., `db_password`, `slack_token`) |
| `encrypted_value` | BLOB | AES-256-GCM encrypted secret value |
| `nonce` | BLOB | 96-bit random nonce used during encryption |
| `created_at` | TEXT | ISO 8601 timestamp of creation |
| `updated_at` | TEXT | ISO 8601 timestamp of last update |

### Encryption Mechanism

When a secret is created or updated:

1. **Generate a random 96-bit nonce** (12 bytes)
2. **Encrypt the plaintext value** using AES-256-GCM with:
   - Key: `VORTEX_SECRET_KEY` (32-byte hex string)
   - Plaintext: User-provided secret value
   - Nonce: Randomly generated per operation
3. **Store encrypted_value + nonce** in the database (plaintext never written)
4. **Create authentication tag** (part of GCM, ensures integrity)

### Key Derivation

VORTEX uses a single master key for all secrets:

```rust
// Environment variable: VORTEX_SECRET_KEY
// Expected format: 32-byte hex string (64 characters)
// Example: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef

let key_bytes = hex::decode(std::env::var("VORTEX_SECRET_KEY"))?;
assert_eq!(key_bytes.len(), 32); // AES-256 requires 32 bytes
```

**Important**: Rotate `VORTEX_SECRET_KEY` regularly (e.g., every 90 days). After rotation, re-encrypt all secrets with the new key.

---

## API Reference

### 1. Create Secret

**Endpoint:** `POST /api/secrets`

Create a new encrypted secret.

**Request Body:**
```json
{
  "name": "db_password",
  "value": "my_super_secret_password"
}
```

**Response (201 Created):**
```json
{
  "id": "secret_7c3f8b2a9d1e4c6f",
  "name": "db_password",
  "created_at": "2026-02-22T22:59:00Z",
  "updated_at": "2026-02-22T22:59:00Z"
}
```

**cURL Example:**
```bash
curl -X POST http://localhost:8080/api/secrets \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{"name": "db_password", "value": "super_secret_123"}'
```

---

### 2. Retrieve Secret

**Endpoint:** `GET /api/secrets/{id}`

Retrieve a secret by ID (decrypts and returns plaintext value).

**Response (200 OK):**
```json
{
  "id": "secret_7c3f8b2a9d1e4c6f",
  "name": "db_password",
  "value": "my_super_secret_password",
  "created_at": "2026-02-22T22:59:00Z",
  "updated_at": "2026-02-22T22:59:00Z"
}
```

**cURL Example:**
```bash
curl -X GET http://localhost:8080/api/secrets/secret_7c3f8b2a9d1e4c6f \
  -H "Authorization: Bearer <token>"
```

---

### 3. Update Secret

**Endpoint:** `PUT /api/secrets/{id}`

Update an existing secret (re-encrypts with a new nonce).

**Request Body:**
```json
{
  "value": "new_secret_password_456"
}
```

**Response (200 OK):**
```json
{
  "id": "secret_7c3f8b2a9d1e4c6f",
  "name": "db_password",
  "created_at": "2026-02-22T22:59:00Z",
  "updated_at": "2026-02-22T23:05:30Z"
}
```

---

### 4. Delete Secret

**Endpoint:** `DELETE /api/secrets/{id}`

Permanently delete a secret.

**Response (204 No Content)**

**cURL Example:**
```bash
curl -X DELETE http://localhost:8080/api/secrets/secret_7c3f8b2a9d1e4c6f \
  -H "Authorization: Bearer <token>"
```

---

### 5. List All Secrets

**Endpoint:** `GET /api/secrets`

List all secrets (returns metadata only; no values).

**Response (200 OK):**
```json
{
  "secrets": [
    {
      "id": "secret_7c3f8b2a9d1e4c6f",
      "name": "db_password",
      "created_at": "2026-02-22T22:59:00Z",
      "updated_at": "2026-02-22T22:59:00Z"
    },
    {
      "id": "secret_a1b2c3d4e5f6g7h8",
      "name": "slack_api_token",
      "created_at": "2026-02-20T10:30:00Z",
      "updated_at": "2026-02-20T10:30:00Z"
    }
  ]
}
```

---

## Task Secret Injection

### How Secrets Are Resolved During Task Dispatch

When a DAG includes a secret reference, VORTEX performs dynamic secret injection:

1. **DAG Definition** includes `secrets` list in task spec
2. **Controller processes DAG**: Resolves secret names to secret IDs
3. **Worker receives task**: Gets decrypted secret values
4. **Secret injected as env var**: Available to container at runtime

### Environment Variable Injection

Secrets are injected with a `SECRET_` prefix (transformed to uppercase):

| Secret Name | Environment Variable |
|------------|----------------------|
| `db_password` | `$SECRET_DB_PASSWORD` |
| `slack_token` | `$SECRET_SLACK_TOKEN` |
| `api_key` | `$SECRET_API_KEY` |

### Example: DAG with Secret References

```yaml
version: "1.0"
name: data-pipeline

secrets:
  - name: db_password
    id: secret_7c3f8b2a9d1e4c6f
  - name: slack_token
    id: secret_a1b2c3d4e5f6g7h8

tasks:
  - name: fetch_data
    image: postgres-client:latest
    command:
      - psql
      - -h
      - db.example.com
      - -U
      - admin
      - -w
      - -c
      - "SELECT * FROM users"
    secrets:
      - db_password  # Injected as $SECRET_DB_PASSWORD
    environment:
      PGPASSWORD: $SECRET_DB_PASSWORD

  - name: notify_slack
    image: python:3.13
    command:
      - python
      - -c
      - "import requests; requests.post(os.getenv('SLACK_WEBHOOK'), json={})"
    secrets:
      - slack_token  # Injected as $SECRET_SLACK_TOKEN
    environment:
      SLACK_WEBHOOK: $SECRET_SLACK_TOKEN
```

### Worker-Side Processing

When a worker receives the task, the controller provides decrypted secrets:

```json
{
  "task_id": "task_123abc",
  "image": "postgres-client:latest",
  "command": [...],
  "secrets": {
    "db_password": "my_super_secret_password",
    "slack_token": "xoxb-token-here"
  }
}
```

The worker injects secrets as environment variables before executing the container:

```bash
export SECRET_DB_PASSWORD="my_super_secret_password"
export SECRET_SLACK_TOKEN="xoxb-token-here"
docker run --env SECRET_DB_PASSWORD --env SECRET_SLACK_TOKEN postgres-client:latest
```

---

## Security Best Practices

### 1. Rotate the Master Key Regularly

- Rotate `VORTEX_SECRET_KEY` every 90 days
- After rotation, re-encrypt all secrets with the new key:
  ```bash
  # Controller provides a rotate-keys endpoint (future enhancement)
  curl -X POST http://localhost:8080/api/secrets/rotate-keys \
    -H "Authorization: Bearer <admin-token>"
  ```

### 2. Use HTTPS for API Calls

- Always use TLS when communicating with the Secrets API
- Disable plaintext HTTP in production
- Example deployment:
  ```bash
  ./vortex --secrets-key $VORTEX_SECRET_KEY --tls-cert /path/to/cert.pem --tls-key /path/to/key.pem
  ```

### 3. Audit Secret Access

- Log all secret access (retrieve, update, delete) with:
  - User/client ID
  - Timestamp
  - Action (GET, PUT, DELETE)
  - Secret name (NOT value)

Example audit log:
```
[2026-02-22T22:59:00Z] user:alice SECRET_GET db_password status:success
[2026-02-22T23:05:30Z] user:bob SECRET_PUT db_password status:success
```

### 4. Never Log Secret Values

- Secrets should never appear in logs, error messages, or stack traces
- Use placeholder values in debugging:
  ```rust
  // ✗ Bad: println!("Secret value: {}", secret_value);
  // ✓ Good: println!("Secret retrieved: {}", secret.name);
  ```

### 5. Restrict Access to VORTEX_SECRET_KEY

- Store `VORTEX_SECRET_KEY` in a secure vault (e.g., HashiCorp Vault, AWS Secrets Manager)
- Only the controller process should have access
- Rotate after any suspected compromise

### 6. Use Read-Only Replicas

- In production, only allow writes to the primary database
- Read replicas can serve retrieve requests (no encryption key needed)
- Separates read and write workloads for performance

---

## Troubleshooting

### Error: "Invalid VORTEX_SECRET_KEY"

**Cause**: The key is not a valid 32-byte hex string.

**Solution**:
```bash
# Generate a new key
openssl rand -hex 32
# Output: a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6
export VORTEX_SECRET_KEY="a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6"
```

### Error: "Decryption failed: authentication tag mismatch"

**Cause**: The ciphertext or nonce was corrupted, or the wrong key was used.

**Solution**:
- Check that `VORTEX_SECRET_KEY` matches the key used during encryption
- Verify database integrity (check for bit flips or corruption)

### Secret Values Not Injected

**Cause**: Secret name doesn't match DAG reference or secret doesn't exist.

**Solution**:
```bash
# List all secrets to verify names
curl http://localhost:8080/api/secrets

# Check DAG task for correct secret references
cat my_dag.yaml | grep -A 5 "secrets:"
```

---

## Related Documentation

- [Pillar 4: Resilience](./PILLAR_4_RESILIENCE.md) — Task recovery when workers fail
- [Architecture Overview](./ARCHITECTURE.md) — System design and data flow
- [API Reference](./API_REFERENCE.md) — Complete endpoint documentation
- [Deployment Guide](./DEPLOYMENT.md) — Setup and configuration
