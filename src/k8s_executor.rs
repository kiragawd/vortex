#![allow(dead_code)]
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, warn};

// ──────────────────────────── K8s Executor ────────────────────────────────────

/// Configuration for the Kubernetes executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sExecutorConfig {
    pub namespace: String,
    pub service_account: Option<String>,
    pub image: String,
    pub image_pull_policy: String,
    pub image_pull_secrets: Vec<String>,
    pub default_resources: K8sResources,
    pub node_selector: HashMap<String, String>,
    pub tolerations: Vec<K8sToleration>,
    pub annotations: HashMap<String, String>,
    pub labels: HashMap<String, String>,
    pub env_from_secrets: Vec<String>,
    pub env_from_configmaps: Vec<String>,
    pub delete_completed_pods: bool,
    pub pod_ttl_seconds: Option<i64>,
}

impl Default for K8sExecutorConfig {
    fn default() -> Self {
        Self {
            namespace: "ryuo".to_string(),
            service_account: None,
            image: "ghcr.io/ryuo/ryuo:latest".to_string(),
            image_pull_policy: "IfNotPresent".to_string(),
            image_pull_secrets: vec![],
            default_resources: K8sResources::default(),
            node_selector: HashMap::new(),
            tolerations: vec![],
            annotations: HashMap::new(),
            labels: HashMap::new(),
            env_from_secrets: vec![],
            env_from_configmaps: vec![],
            delete_completed_pods: true,
            pod_ttl_seconds: Some(3600),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sResources {
    pub cpu_request: String,
    pub memory_request: String,
    pub cpu_limit: String,
    pub memory_limit: String,
}

impl Default for K8sResources {
    fn default() -> Self {
        Self {
            cpu_request: "100m".to_string(),
            memory_request: "128Mi".to_string(),
            cpu_limit: "500m".to_string(),
            memory_limit: "512Mi".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sToleration {
    pub key: String,
    pub operator: String,
    pub value: Option<String>,
    pub effect: Option<String>,
}

/// Represents a task pod that the K8s executor manages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPod {
    pub pod_name: String,
    pub namespace: String,
    pub dag_id: String,
    pub task_id: String,
    pub run_id: String,
    pub status: PodStatus,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PodStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

/// K8s executor that launches one pod per task.
pub struct K8sExecutor {
    config: K8sExecutorConfig,
    // In production, this would hold a kube::Client
}

impl K8sExecutor {
    pub fn new(config: K8sExecutorConfig) -> Self {
        info!(namespace = %config.namespace, image = %config.image, "k8s_executor_initialized");
        Self { config }
    }

    /// Generate the pod spec JSON for a task.
    pub fn generate_pod_spec(
        &self,
        dag_id: &str,
        task_id: &str,
        run_id: &str,
        command: &[String],
        env_vars: &HashMap<String, String>,
        resources: Option<&K8sResources>,
    ) -> serde_json::Value {
        let res = resources.unwrap_or(&self.config.default_resources);
        let pod_name = format!("ryuo-{}-{}-{}", dag_id, task_id, &run_id[..8.min(run_id.len())]);
        let sanitized_name: String = pod_name.chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
            .take(63)
            .collect();
        // Ensure pod name starts and ends with alphanumeric (K8s requirement)
        let sanitized_name = sanitized_name.trim_matches('-').to_string();

        let mut labels = self.config.labels.clone();
        labels.insert("app.kubernetes.io/managed-by".into(), "ryuo".into());
        labels.insert("ryuo/dag-id".into(), dag_id.to_string());
        labels.insert("ryuo/task-id".into(), task_id.to_string());
        labels.insert("ryuo/run-id".into(), run_id.to_string());

        let env: Vec<serde_json::Value> = env_vars.iter()
            .map(|(k, v)| serde_json::json!({"name": k, "value": v}))
            .chain(vec![
                serde_json::json!({"name": "RYUO_DAG_ID", "value": dag_id}),
                serde_json::json!({"name": "RYUO_TASK_ID", "value": task_id}),
                serde_json::json!({"name": "RYUO_RUN_ID", "value": run_id}),
            ])
            .collect();

        let mut env_from = Vec::new();
        for secret in &self.config.env_from_secrets {
            env_from.push(serde_json::json!({"secretRef": {"name": secret}}));
        }
        for cm in &self.config.env_from_configmaps {
            env_from.push(serde_json::json!({"configMapRef": {"name": cm}}));
        }

        let mut pod_spec = serde_json::json!({
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": sanitized_name,
                "namespace": self.config.namespace,
                "labels": labels,
                "annotations": self.config.annotations,
            },
            "spec": {
                "restartPolicy": "Never",
                "containers": [{
                    "name": "task",
                    "image": self.config.image,
                    "imagePullPolicy": self.config.image_pull_policy,
                    "command": command,
                    "env": env,
                    "envFrom": env_from,
                    "resources": {
                        "requests": {
                            "cpu": res.cpu_request,
                            "memory": res.memory_request,
                        },
                        "limits": {
                            "cpu": res.cpu_limit,
                            "memory": res.memory_limit,
                        },
                    },
                }],
            }
        });

        if let Some(sa) = &self.config.service_account {
            pod_spec["spec"]["serviceAccountName"] = serde_json::json!(sa);
        }
        if !self.config.node_selector.is_empty() {
            pod_spec["spec"]["nodeSelector"] = serde_json::json!(self.config.node_selector);
        }
        if !self.config.image_pull_secrets.is_empty() {
            let ips: Vec<_> = self.config.image_pull_secrets.iter()
                .map(|s| serde_json::json!({"name": s}))
                .collect();
            pod_spec["spec"]["imagePullSecrets"] = serde_json::json!(ips);
        }
        if let Some(ttl) = self.config.pod_ttl_seconds {
            pod_spec["spec"]["activeDeadlineSeconds"] = serde_json::json!(ttl);
        }

        pod_spec
    }

    // STUB(kube-rs): Replace with real kube::Client pod creation when kube dependency is added
    /// Submit a task for execution as a K8s pod (placeholder — requires kube client).
    pub async fn submit_task(
        &self,
        dag_id: &str,
        task_id: &str,
        run_id: &str,
        command: &[String],
        env_vars: &HashMap<String, String>,
    ) -> Result<TaskPod> {
        let pod_spec = self.generate_pod_spec(dag_id, task_id, run_id, command, env_vars, None);
        let pod_name = pod_spec["metadata"]["name"].as_str().unwrap_or("unknown").to_string();
        warn!(pod = %pod_name, dag_id = %dag_id, task_id = %task_id, "submit_task() is a stub — pod not actually created in K8s");

        // In production: kube::Client::try_default().await?.create(&pp, &pod).await?
        // For now, return a placeholder TaskPod
        Ok(TaskPod {
            pod_name,
            namespace: self.config.namespace.clone(),
            dag_id: dag_id.to_string(),
            task_id: task_id.to_string(),
            run_id: run_id.to_string(),
            status: PodStatus::Pending,
            started_at: None,
            finished_at: None,
        })
    }

    // TODO(stub): Replace with real kube::Client pod status query
    /// Get the status of a task pod (placeholder).
    pub async fn get_pod_status(&self, pod_name: &str) -> Result<PodStatus> {
        warn!(pod = %pod_name, "get_pod_status() is a stub — returning PodStatus::Unknown");
        // In production: read pod status from K8s API
        Ok(PodStatus::Unknown)
    }

    /// Delete a completed pod (cleanup).
    pub async fn delete_pod(&self, pod_name: &str) -> Result<()> {
        info!(pod = %pod_name, "k8s_pod_delete");
        // In production: kube::Client delete pod
        Ok(())
    }

    /// ENT-16: Validate inputs and attempt pod submission with environment checks.
    /// Returns the sanitized pod name on success.
    pub async fn submit_pod(
        &self,
        dag_id: &str,
        task_id: &str,
        run_id: &str,
    ) -> Result<String> {
        if dag_id.is_empty() || task_id.is_empty() {
            anyhow::bail!("K8s executor: dag_id and task_id are required for pod naming");
        }

        // Inline sanitization to avoid scope resolution ambiguity (same logic as sanitize_k8s_name)
        let sanitize = |s: &str| -> String {
            let r: String = s.chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
                .collect();
            r.trim_matches('-').to_string()
        };
        let pod_name = format!(
            "ryuo-{}-{}-{}",
            sanitize(dag_id),
            sanitize(task_id),
            &run_id[..8.min(run_id.len())]
        );

        // Check for Kubernetes configuration
        let has_kubeconfig = std::env::var("KUBECONFIG").is_ok();
        let has_in_cluster = std::env::var("KUBERNETES_SERVICE_HOST").is_ok();
        if !has_kubeconfig && !has_in_cluster {
            anyhow::bail!(
                "K8s executor: no Kubernetes configuration found. \
                 Set KUBECONFIG for out-of-cluster access or run inside a Kubernetes pod \
                 (KUBERNETES_SERVICE_HOST must be set)."
            );
        }

        info!(pod = %pod_name, namespace = %self.config.namespace, "ENT-16: k8s_submit_pod");
        // TODO(ENT-16): Replace with real kube::Client pod submission once
        // the `kube` crate is added to Cargo.toml:
        //   kube = { version = "0.88", features = ["runtime", "derive"] }
        //   k8s-openapi = { version = "0.21", features = ["v1_28"] }
        Ok(pod_name)
    }

    /// ENT-16: Lightweight connectivity check using the Kubernetes API server URL.
    pub async fn health_check(&self) -> Result<()> {
        let api_server = std::env::var("KUBERNETES_SERVICE_HOST")
            .map(|h| {
                let port = std::env::var("KUBERNETES_SERVICE_PORT").unwrap_or_else(|_| "443".into());
                format!("https://{}:{}", h, port)
            })
            .or_else(|_| std::env::var("KUBECONFIG").map(|_| "kubeconfig".to_string()))
            .unwrap_or_else(|_| "not configured".to_string());
        info!(api_server = %api_server, "ENT-16: k8s_health_check");
        // TODO(ENT-16): perform actual /healthz request once kube crate is linked
        Ok(())
    }
}

// ──────────────────────── External Secrets Engine ─────────────────────────────

/// Supported external secret backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecretBackend {
    Vault { addr: String, mount: String, role: Option<String> },
    AwsSecretsManager { region: String },
    GcpSecretManager { project: String },
    AzureKeyVault { vault_url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalSecretRef {
    pub backend: String,        // "vault", "aws", "gcp", "azure"
    pub path: String,           // secret path in the backend
    pub key: Option<String>,    // specific key within the secret
    pub version: Option<String>,
}

/// External secrets manager that syncs secrets from external backends.
pub struct ExternalSecretsManager {
    backends: HashMap<String, SecretBackend>,
}

impl ExternalSecretsManager {
    pub fn new() -> Self {
        Self { backends: HashMap::new() }
    }

    pub fn register_backend(&mut self, name: &str, backend: SecretBackend) {
        info!(backend = %name, "external_secret_backend_registered");
        self.backends.insert(name.to_string(), backend);
    }

    /// Resolve an external secret reference to its value (placeholder).
    pub async fn resolve_secret(&self, secret_ref: &ExternalSecretRef) -> Result<String> {
        let backend = self.backends.get(&secret_ref.backend)
            .context(format!("External secret backend '{}' not configured", secret_ref.backend))?;

        match backend {
            SecretBackend::Vault { addr, mount, .. } => {
                info!(addr = %addr, mount = %mount, path = %secret_ref.path, "vault_secret_fetch");
                // In production: use reqwest to call Vault HTTP API
                // GET {addr}/v1/{mount}/data/{path}
                Ok(format!("vault_placeholder_{}", secret_ref.path))
            }
            SecretBackend::AwsSecretsManager { region } => {
                info!(region = %region, path = %secret_ref.path, "aws_secret_fetch");
                // In production: use aws-sdk-secretsmanager
                Ok(format!("aws_placeholder_{}", secret_ref.path))
            }
            SecretBackend::GcpSecretManager { project } => {
                info!(project = %project, path = %secret_ref.path, "gcp_secret_fetch");
                // In production: use google-cloud-secretmanager
                Ok(format!("gcp_placeholder_{}", secret_ref.path))
            }
            SecretBackend::AzureKeyVault { vault_url } => {
                info!(vault = %vault_url, path = %secret_ref.path, "azure_secret_fetch");
                // In production: use azure_security_keyvault
                Ok(format!("azure_placeholder_{}", secret_ref.path))
            }
        }
    }

    /// List registered backends.
    pub fn list_backends(&self) -> Vec<String> {
        self.backends.keys().cloned().collect()
    }
}

/// ENT-16: Sanitize a string to a valid Kubernetes name component.
/// Lowercases, replaces non-alphanumeric-or-hyphen chars with hyphens, strips leading/trailing hyphens.
pub fn sanitize_k8s_name(input: &str) -> String {
    let s: String = input
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect();
    s.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pod_spec_generation() {
        let config = K8sExecutorConfig::default();
        let executor = K8sExecutor::new(config);
        let spec = executor.generate_pod_spec(
            "my_dag", "my_task", "run-12345678",
            &["python".to_string(), "script.py".to_string()],
            &HashMap::new(),
            None,
        );
        assert_eq!(spec["kind"], "Pod");
        assert!(spec["metadata"]["name"].as_str().unwrap().starts_with("ryuo-my-dag-my-task-"));
        assert_eq!(spec["metadata"]["labels"]["ryuo/dag-id"], "my_dag");
        assert_eq!(spec["spec"]["containers"][0]["image"], "ghcr.io/ryuo/ryuo:latest");
    }

    #[test]
    fn test_external_secrets_manager() {
        let mut mgr = ExternalSecretsManager::new();
        mgr.register_backend("vault", SecretBackend::Vault {
            addr: "https://vault.example.com".into(),
            mount: "secret".into(),
            role: None,
        });
        assert_eq!(mgr.list_backends(), vec!["vault"]);
    }

    #[test]
    fn test_k8s_resources_default() {
        let res = K8sResources::default();
        assert_eq!(res.cpu_request, "100m");
        assert_eq!(res.memory_limit, "512Mi");
    }
}
