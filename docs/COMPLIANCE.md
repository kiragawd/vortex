# Compliance, Governance & Change Management

## Overview

Vortex provides enterprise compliance features including audit logging, approval workflows, data retention policies, and regulatory control mapping.

**Module:** `src/compliance.rs`

---

## Audit Logging

Comprehensive event tracking for all user and system actions.

### Event Types

| Category | Examples |
|----------|---------|
| Authentication | Login, logout, failed login attempts |
| DAG operations | Create, update, delete, trigger, pause, unpause |
| Secret management | Create, update, delete, access |
| User management | Create, update, delete, role changes |
| System events | Startup, shutdown, migration, failover |

### Audit Entry Structure

Each `AuditEntry` records:
- **Timestamp** — When the action occurred
- **Actor** — User or system component performing the action
- **Action** — Type of operation
- **Resource** — Target object (DAG ID, secret key, user ID)
- **Details** — JSON metadata with request context
- **Source IP** — Client IP address

### Database Schema

| Table | Purpose |
|-------|---------|
| `audit_log` | Permanent trail of all security and operational events |

### CLI

```bash
# Query audit logs
vortex-cli audit query --action "dag.trigger" --limit 100

# Export audit data
vortex-cli audit export --format json --output audit_export.json
```

---

## Approval Workflows

DAG change approval gates for change management in regulated environments.

### Lifecycle

```
Request → Pending → Approved / Rejected
```

1. **Request** — User submits a DAG change (create, update, or delete)
2. **Pending** — Change is held in approval queue
3. **Approved** — Approver grants the change; DAG update is applied
4. **Rejected** — Approver denies the change; no modification is made

### Key Types

- `ApprovalGate` — Defines which DAG operations require approval and who can approve
- `ApprovalRequest` — Individual approval request with status, requester, approver, and comments

### Database Schema

| Table | Purpose |
|-------|---------|
| `approval_gates` | Approval gate definitions per DAG or globally |
| `approval_requests` | Individual approval request records |

---

## Retention Policies

Configurable time-based and count-based retention for logs and run history.

### Policy Types

| Type | Description |
|------|-------------|
| **Time-based** | Delete records older than N days |
| **Count-based** | Keep only the most recent N records |

### Targets

| Target | Description |
|--------|-------------|
| DAG runs | Historical execution records |
| Task instances | Individual task execution records |
| Audit logs | Security and operational event log |
| Lineage events | Data lineage tracking records |

### Database Schema

| Table | Purpose |
|-------|---------|
| `retention_policies` | Retention configuration per target type |

---

## Compliance Tracking

Regulatory control mapping and assessment for enterprise compliance requirements.

### Supported Frameworks

| Framework | Description |
|-----------|-------------|
| **SOC 2** | Service Organization Control — Trust Services Criteria |
| **GDPR** | General Data Protection Regulation |
| **HIPAA** | Health Insurance Portability and Accountability Act |

### Control Mapping

Each compliance control maps to specific Vortex capabilities:

| Control Area | Vortex Feature |
|-------------|----------------|
| Access control | RBAC, team isolation, API token scoping |
| Audit trails | Audit logging with immutable event records |
| Data protection | AES-256-GCM vault, TLS transport |
| Change management | Approval workflows, DAG versioning |
| Incident response | PagerDuty integration, alerting |
| Data retention | Configurable retention policies |

### Database Schema

| Table | Purpose |
|-------|---------|
| `compliance_controls` | Control definitions and assessment status |

### CLI

```bash
# List compliance controls
vortex-cli compliance list

# Create a new control
vortex-cli compliance create --framework "SOC2" --control "CC6.1" --description "Logical access controls"

# Update control status
vortex-cli compliance update --id <control_id> --status "implemented"

# Run compliance check
vortex-cli compliance check --framework "SOC2"
```

---

## Related Documentation

- [Authentication](./AUTHENTICATION.md) — IAM, RBAC, and security model
- [Secrets Vault](./SECRETS_VAULT.md) — Encrypted secret management
- [Observability](./OBSERVABILITY.md) — Lineage tracking and incident management
