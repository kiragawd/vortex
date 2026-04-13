const BASE_URL = '/api';

interface RequestOptions {
  method?: string;
  body?: unknown;
  headers?: Record<string, string>;
}

export interface PaginatedResponse<T> {
  data: T[];
  total: number;
  limit: number;
  offset: number;
}

async function request<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const { method = 'GET', body, headers = {} } = opts;
  const url = `${BASE_URL}${path}`;
  const token = localStorage.getItem('ryuo_token');

  const res = await fetch(url, {
    method,
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...headers,
    },
    ...(body ? { body: JSON.stringify(body) } : {}),
  });

  if (res.status === 401) {
    localStorage.removeItem('ryuo_token');
    window.location.href = '/login';
    throw new Error('Unauthorized');
  }

  if (!res.ok) {
    const text = await res.text().catch(() => 'Unknown error');
    throw new Error(`${res.status}: ${text}`);
  }

  if (res.status === 204) return undefined as T;
  return res.json();
}

export const api = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body?: unknown) => request<T>(path, { method: 'POST', body }),
  put: <T>(path: string, body?: unknown) => request<T>(path, { method: 'PUT', body }),
  patch: <T>(path: string, body?: unknown) => request<T>(path, { method: 'PATCH', body }),
  delete: <T>(path: string) => request<T>(path, { method: 'DELETE' }),
};

// --- Auth ---
export interface LoginResponse {
  api_key: string;
  role: string;
  username: string;
}

export const authApi = {
  login: (username: string, password: string) =>
    api.post<LoginResponse>('/login', { username, password }),
  providers: () => api.get<{ providers: unknown[] }>('/auth/providers'),
};

// --- DAGs ---
export interface Dag {
  id: string;
  schedule_interval: string | null;
  is_paused: boolean;
  last_run: string | null;
  next_run: string | null;
  created_at: string;
  team_id: string | null;
}

export interface DagTask {
  id: string;
  name: string;
  command?: string;
  task_type?: string;
  config?: Record<string, unknown>;
  max_retries?: number;
  retry_delay_secs?: number;
  pool?: string;
  task_group?: string | null;
  execution_timeout?: number | null;
}

export interface TaskInstance {
  id: string;
  task_id: string;
  state: string;
  execution_date: string;
  start_time: string | null;
  end_time: string | null;
  stdout?: string | null;
  stderr?: string | null;
  duration_ms?: number | null;
  retry_count: number;
  run_id: string | null;
}

export interface DagTasksResponse {
  dag_id: string;
  tasks: DagTask[];
  instances: TaskInstance[];
  instances_total: number;
  dependencies: [string, string][]; // [upstream_name, downstream_name]
  dag?: Dag;
}

export interface DagSource {
  dag_id: string;
  source: string;
  file_path: string;
}

export const dagsApi = {
  list: () => api.get<PaginatedResponse<Dag>>('/dags').then((r) => r.data),
  /** No GET /api/dags/:id endpoint — use getTasks which includes `dag` in response */
  get: (id: string) =>
    api.get<DagTasksResponse>(`/dags/${encodeURIComponent(id)}/tasks`).then((r) => r.dag as Dag),
  trigger: (id: string) => api.post<{ message: string }>(`/dags/${encodeURIComponent(id)}/trigger`),
  retry: (id: string) => api.post<{ message: string }>(`/dags/${encodeURIComponent(id)}/retry`),
  getTasks: (id: string) => api.get<DagTasksResponse>(`/dags/${encodeURIComponent(id)}/tasks`),
  getRuns: (id: string, limit = 20) =>
    api.get<PaginatedResponse<DagRun>>(`/dags/${encodeURIComponent(id)}/runs?limit=${limit}`).then((r) => r.data),
  getSource: (id: string) => api.get<DagSource>(`/dags/${encodeURIComponent(id)}/source`),
  /** Update source and reparse as a Python DAG */
  updateSource: (id: string, source: string) =>
    api.patch<{ message: string }>(`/dags/${encodeURIComponent(id)}/source`, { source }),
  /** Update source and reparse as a Rust/Config DAG */
  updateSourceRust: (id: string, source: string) =>
    api.patch<{ message: string }>(`/dags/${encodeURIComponent(id)}/source/rust`, { source }),
};

// --- Runs ---
export interface DagRun {
  id: string;
  dag_id: string;
  state: string;
  execution_date: string;
  start_time: string | null;
  end_time: string | null;
  triggered_by: string;
  sla_missed: boolean;
}

export const runsApi = {
  list: () => api.get<PaginatedResponse<DagRun>>('/runs').then((r) => r.data),
  retry: (dagId: string) => api.post(`/dags/${encodeURIComponent(dagId)}/retry`),
};

// --- Health ---
export interface HealthStatus {
  status: string;
  version: string;
  db: string;
}

export const healthApi = {
  check: () => api.get<HealthStatus>('/health'),
};

// --- Swarm / Workers ---
export interface SwarmStatus {
  enabled: boolean;
  active_workers: number;
  queue_depth: number;
}

export interface SwarmWorker {
  worker_id: string;
  hostname?: string;
  status?: string;
  address?: string;
  capacity?: number;
  active_tasks?: number;
  labels?: string;
  current_task?: string | null;
  last_heartbeat?: string | null;
}

export const swarmApi = {
  status: () => api.get<SwarmStatus>('/swarm/status'),
  workers: () =>
    api.get<PaginatedResponse<SwarmWorker>>('/swarm/workers').then((r) => r.data),
  drainWorker: (id: string) =>
    api.post(`/swarm/workers/${encodeURIComponent(id)}/drain`),

};

// --- Compliance ---
export interface AuditEntry {
  id: number;
  action: string;
  actor: string;
  resource_type: string | null;
  resource_id: string | null;
  created_at: string;
  actor_ip?: string | null;
  details?: Record<string, unknown>;
  event_type?: string | null;
  team_id?: string | null;
}

export interface ApprovalGate {
  id: string;
  name: string;
  resource_type: string;
  resource_pattern: string;
  required_approvers: number;
  approver_roles: string[];
  enabled: boolean;
}

export interface ApprovalRequest {
  id: string;
  gate_id: string;
  requested_by: string;
  resource_type: string;
  resource_id: string;
  status: string;
  change_description?: string;
  created_at: string;
}

export const complianceApi = {
  auditLog: (limit = 100) =>
    api.get<{ entries: AuditEntry[] }>(`/audit/log?limit=${limit}`).then((r) => r.entries ?? []),
  approvalGates: () =>
    api.get<{ gates: ApprovalGate[] }>('/approval/gates').then((r) => r.gates ?? []),
  approvalRequests: (status?: string) =>
    api
      .get<{ requests: ApprovalRequest[] }>(
        `/approval/requests${status ? `?status=${encodeURIComponent(status)}` : ''}`,
      )
      .then((r) => r.requests ?? []),
  approveRequest: (id: string, comment?: string) =>
    api.post(`/approval/requests/${encodeURIComponent(id)}/approve`, { comment }),
  rejectRequest: (id: string, comment?: string) =>
    api.post(`/approval/requests/${encodeURIComponent(id)}/reject`, { comment }),
  complianceSummary: (framework: string) =>
    api.get(`/compliance/summary/${encodeURIComponent(framework)}`),
};

// --- RBAC ---
export interface RbacRole {
  id: string;
  name: string;
  description: string;
  is_system?: boolean;
}

export interface RbacPermission {
  id: number;
  resource: string;
  action: string;
}

export interface ApiToken {
  id: string;
  name: string;
  username: string;
  scopes: string[];
  team_id?: string | null;
  created_at?: string;
  expires_at?: string | null;
  revoked?: boolean;
}

export interface IpAllowlistRule {
  id: string;
  cidr: string;
  description?: string | null;
  enabled: boolean;
  created_at?: string;
}

export const rbacApi = {
  roles: () =>
    api.get<{ roles: RbacRole[] }>('/rbac/roles').then((r) => r.roles ?? []),
  rolePermissions: (roleId: string) =>
    api
      .get<{ permissions: RbacPermission[] }>(`/rbac/roles/${encodeURIComponent(roleId)}/permissions`)
      .then((r) => r.permissions ?? []),
  tokens: () =>
    api.get<{ tokens: ApiToken[] }>('/tokens').then((r) => r.tokens ?? []),
  createToken: (body: { name: string; scopes: string[]; team_id?: string; expires_at?: string }) =>
    api.post<{ id: string; token: string; name: string }>('/tokens', body),
  revokeToken: (id: string) =>
    api.post(`/tokens/${encodeURIComponent(id)}/revoke`),
  ipAllowlist: () =>
    api.get<{ rules: IpAllowlistRule[] }>('/network/ip-allowlist').then((r) => r.rules ?? []),
  addIpRule: (cidr: string, description?: string) =>
    api.post('/network/ip-allowlist', {
      id: crypto.randomUUID(),
      cidr,
      description: description ?? '',
      enabled: true,
    }),
  deleteIpRule: (id: string) =>
    api.delete(`/network/ip-allowlist/${encodeURIComponent(id)}`),
};

// --- Incidents ---
export interface IncidentConfig {
  id: string;
  name: string;
  channel: string; // "pagerduty" | "slack" | "email" | "webhook"
  enabled: boolean;
  config: Record<string, unknown>;
  severity_filter?: string | null;
  dag_filter?: string | null;
  created_at?: string;
}

export const incidentApi = {
  configs: () =>
    api
      .get<{ configs: IncidentConfig[] }>('/incidents/configs')
      .then((r) => r.configs ?? []),
  createConfig: (body: Omit<IncidentConfig, 'id' | 'created_at'>) =>
    api.post<IncidentConfig>('/incidents/configs', body),
  deleteConfig: (id: string) =>
    api.delete(`/incidents/configs/${encodeURIComponent(id)}`),
};

// --- Lineage ---
export interface LineageDataset {
  id?: string;
  namespace: string;
  name: string;
  facets?: Record<string, unknown>;
  created_at?: string;
}

export const lineageApi = {
  datasets: (limit = 50) =>
    api
      .get<{ datasets: LineageDataset[] }>(`/lineage/datasets?limit=${limit}`)
      .then((r) => r.datasets ?? []),
  events: (dagId: string, limit = 50) =>
    api
      .get<{ events: unknown[] }>(
        `/lineage/events/${encodeURIComponent(dagId)}?limit=${limit}`,
      )
      .then((r) => r.events ?? []),
};
