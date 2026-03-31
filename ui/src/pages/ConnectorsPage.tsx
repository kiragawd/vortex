import { useState } from 'react';
import { Cloud, CheckCircle, AlertCircle, Database, BarChart2, BookOpen, Waves, ArrowLeft } from 'lucide-react';

interface ConnectorDoc {
  overview: string;
  authentication: string;
  capabilities: string[];
  configExample: string;
  dagExample: string;
}

interface Connector {
  name: string;
  category: string;
  description: string;
  icon: React.ElementType;
  color: string;
  status: 'available' | 'beta' | 'experimental';
  docs: ConnectorDoc;
}

const CONNECTORS: Connector[] = [
  {
    name: 'BigQuery',
    category: 'Cloud Data Warehouse',
    description: 'Read and write Google BigQuery tables with partition support and job-level monitoring.',
    icon: Database,
    color: 'bg-blue-50 text-blue-600 dark:bg-blue-950/30 dark:text-blue-400',
    status: 'available',
    docs: {
      overview: 'The BigQuery connector provides full read/write access to Google BigQuery datasets. It supports partitioned tables, clustered tables, and BigQuery job-level monitoring with automatic cost estimation.',
      authentication: 'Requires a GCP service account JSON key stored in the Vortex vault under the key `gcp_service_account`. Alternatively, set `GOOGLE_APPLICATION_CREDENTIALS` environment variable.',
      capabilities: ['BatchRead', 'BatchWrite', 'AsyncJobs', 'PushdownPredicates', 'Partitioned Tables', 'Cost Estimation'],
      configExample: `connector:
  type: bigquery
  project_id: my-gcp-project
  dataset: analytics
  credentials_vault_key: gcp_service_account
  location: US
  timeout_ms: 30000`,
      dagExample: `- task: load_analytics
  operator: bigquery
  config:
    query: "SELECT * FROM \`project.dataset.table\` WHERE date = CURRENT_DATE()"
    destination_table: project.dataset.daily_summary
    write_disposition: WRITE_TRUNCATE`,
    },
  },
  {
    name: 'Amazon Redshift',
    category: 'Cloud Data Warehouse',
    description: 'Execute queries, COPY/UNLOAD operations, and cluster management on Amazon Redshift.',
    icon: Database,
    color: 'bg-orange-50 text-orange-600 dark:bg-orange-950/30 dark:text-orange-400',
    status: 'available',
    docs: {
      overview: 'The Amazon Redshift connector supports SQL query execution, COPY/UNLOAD operations for bulk data movement, and cluster management. It integrates with AWS IAM roles for secure cross-account access.',
      authentication: 'Requires Redshift credentials stored in the Vortex vault: `redshift_host`, `redshift_user`, `redshift_password`, `redshift_database`. Supports IAM-based authentication via `redshift_iam_role`.',
      capabilities: ['BatchRead', 'BatchWrite', 'Transactions', 'COPY/UNLOAD', 'IAM Auth', 'Cluster Management'],
      configExample: `connector:
  type: redshift
  host_vault_key: redshift_host
  database: analytics
  user_vault_key: redshift_user
  password_vault_key: redshift_password
  port: 5439
  ssl_mode: require`,
      dagExample: `- task: unload_to_s3
  operator: redshift
  config:
    query: "UNLOAD ('SELECT * FROM staging.events') TO 's3://bucket/prefix/' IAM_ROLE 'arn:aws:iam::role/RedshiftS3'"
    timeout_ms: 120000`,
    },
  },
  {
    name: 'Apache Kafka',
    category: 'Streaming',
    description: 'Produce and consume Kafka topics with configurable serialization and consumer group management.',
    icon: Waves,
    color: 'bg-purple-50 text-purple-600 dark:bg-purple-950/30 dark:text-purple-400',
    status: 'available',
    docs: {
      overview: 'The Apache Kafka connector enables producing to and consuming from Kafka topics. It supports Avro/JSON/Protobuf serialization, consumer group management, and offset tracking for exactly-once processing semantics.',
      authentication: 'Supports SASL/PLAIN, SASL/SCRAM, and mTLS authentication. Store credentials in the Vortex vault under `kafka_username`, `kafka_password`, or provide TLS certificate paths.',
      capabilities: ['Produce', 'Consume', 'Consumer Groups', 'Avro/JSON/Protobuf', 'Exactly-Once', 'Offset Tracking'],
      configExample: `connector:
  type: kafka
  bootstrap_servers: broker1:9092,broker2:9092
  security_protocol: SASL_SSL
  sasl_mechanism: SCRAM-SHA-256
  username_vault_key: kafka_username
  password_vault_key: kafka_password`,
      dagExample: `- task: publish_events
  operator: kafka_produce
  config:
    topic: analytics.events
    serialization: json
    key_field: event_id
    acks: all`,
    },
  },
  {
    name: 'Amazon S3',
    category: 'Object Storage',
    description: 'Transfer files, list objects, copy across buckets, and trigger downstream tasks on S3 events.',
    icon: Cloud,
    color: 'bg-amber-50 text-amber-600 dark:bg-amber-950/30 dark:text-amber-400',
    status: 'available',
    docs: {
      overview: 'The Amazon S3 connector provides file transfer, object listing, cross-bucket copy, and S3 event-triggered task execution. It supports multipart uploads, server-side encryption, and lifecycle management.',
      authentication: 'Uses AWS credentials from the Vortex vault (`aws_access_key_id`, `aws_secret_access_key`) or IAM instance roles. Supports cross-account access via `sts:AssumeRole`.',
      capabilities: ['Upload', 'Download', 'List', 'Copy', 'Multipart Upload', 'Server-Side Encryption', 'Event Triggers'],
      configExample: `connector:
  type: s3
  region: us-east-1
  access_key_vault_key: aws_access_key_id
  secret_key_vault_key: aws_secret_access_key
  endpoint_url: null  # optional, for S3-compatible stores`,
      dagExample: `- task: upload_report
  operator: s3_upload
  config:
    bucket: reports-bucket
    key: "daily/{{ ds }}/report.parquet"
    local_path: /tmp/report.parquet
    server_side_encryption: AES256`,
    },
  },
  {
    name: 'Google Cloud Storage',
    category: 'Object Storage',
    description: 'Upload, download, and manage GCS objects with automatic retry and checksum validation.',
    icon: Cloud,
    color: 'bg-sky-50 text-sky-600 dark:bg-sky-950/30 dark:text-sky-400',
    status: 'available',
    docs: {
      overview: 'The Google Cloud Storage connector handles upload, download, and object management with automatic retry, CRC32C checksum validation, and resumable uploads for large files.',
      authentication: 'Requires a GCP service account JSON key in the Vortex vault under `gcp_service_account`, or uses Application Default Credentials.',
      capabilities: ['Upload', 'Download', 'List', 'Resumable Uploads', 'Checksum Validation', 'Lifecycle Management'],
      configExample: `connector:
  type: gcs
  project_id: my-gcp-project
  credentials_vault_key: gcp_service_account
  default_bucket: data-lake-bucket`,
      dagExample: `- task: download_data
  operator: gcs_download
  config:
    bucket: data-lake-bucket
    object: "raw/{{ ds }}/events.csv"
    local_path: /tmp/events.csv
    checksum_validation: true`,
    },
  },
  {
    name: 'Delta Lake',
    category: 'Lakehouse',
    description: 'Read/write Delta tables with schema evolution, ACID transactions, and time-travel queries.',
    icon: Database,
    color: 'bg-emerald-50 text-emerald-600 dark:bg-emerald-950/30 dark:text-emerald-400',
    status: 'available',
    docs: {
      overview: 'The Delta Lake connector provides read/write access to Delta tables with full support for schema evolution, ACID transactions, time-travel queries, and Z-ordering optimization.',
      authentication: 'Uses cloud storage credentials (S3/GCS/ADLS) from the Vortex vault. For Databricks-managed Delta tables, provide a Databricks personal access token.',
      capabilities: ['BatchRead', 'BatchWrite', 'ACID Transactions', 'Schema Evolution', 'Time Travel', 'Z-Ordering', 'Partition Pruning'],
      configExample: `connector:
  type: delta_lake
  storage_path: s3://lakehouse/delta/
  aws_access_key_vault_key: aws_access_key_id
  aws_secret_key_vault_key: aws_secret_access_key
  region: us-east-1`,
      dagExample: `- task: merge_daily
  operator: delta_merge
  config:
    target_table: s3://lakehouse/delta/events
    source_query: "SELECT * FROM staging WHERE date = '{{ ds }}'"
    merge_condition: "target.id = source.id"
    schema_evolution: true`,
    },
  },
  {
    name: 'Snowflake',
    category: 'Cloud Data Warehouse',
    description: 'Query execution, data loading/unloading, warehouse sizing, and result caching.',
    icon: BarChart2,
    color: 'bg-cyan-50 text-cyan-600 dark:bg-cyan-950/30 dark:text-cyan-400',
    status: 'beta',
    docs: {
      overview: 'The Snowflake connector supports SQL query execution, COPY INTO data loading/unloading, warehouse auto-scaling, and result caching. Supports both REST API and SnowSQL SDK modes.',
      authentication: 'Supports username/password, key-pair authentication (RSA), and OAuth. Store credentials in the Vortex vault: `snowflake_account`, `snowflake_user`, `snowflake_private_key` or `snowflake_password`.',
      capabilities: ['BatchRead', 'BatchWrite', 'AsyncJobs', 'ArrowZeroCopy', 'Warehouse Management', 'Key-Pair Auth', 'Result Caching'],
      configExample: `connector:
  type: snowflake
  account_vault_key: snowflake_account
  user_vault_key: snowflake_user
  private_key_vault_key: snowflake_private_key
  warehouse: COMPUTE_WH
  database: ANALYTICS
  schema: PUBLIC
  role: SYSADMIN`,
      dagExample: `- task: run_transform
  operator: snowflake
  config:
    query: "CALL analytics.transform_daily('{{ ds }}')"
    warehouse: COMPUTE_WH
    timeout_ms: 300000
    retry_on_timeout: true`,
    },
  },
  {
    name: 'dbt',
    category: 'Transformation',
    description: 'Trigger dbt Cloud jobs, run local dbt projects, and capture test results as DAG outputs.',
    icon: BookOpen,
    color: 'bg-rose-50 text-rose-600 dark:bg-rose-950/30 dark:text-rose-400',
    status: 'beta',
    docs: {
      overview: 'The dbt connector integrates with dbt Cloud and dbt Core. It can trigger dbt Cloud jobs, run local dbt projects, capture test results, and expose dbt model metadata as Vortex lineage events.',
      authentication: 'For dbt Cloud: store API token in vault as `dbt_cloud_token` with account ID. For dbt Core: provide the path to the dbt project directory and profiles.yml.',
      capabilities: ['dbt Cloud Jobs', 'dbt Core Run', 'Test Results', 'Lineage Integration', 'Model Metadata', 'Freshness Checks'],
      configExample: `connector:
  type: dbt
  mode: cloud  # or "core"
  # dbt Cloud settings
  api_token_vault_key: dbt_cloud_token
  account_id: "12345"
  # dbt Core settings (when mode: core)
  # project_dir: /opt/dbt/my_project
  # profiles_dir: /opt/dbt/.dbt`,
      dagExample: `- task: run_dbt_models
  operator: dbt
  config:
    mode: cloud
    job_id: "67890"
    wait_for_completion: true
    capture_run_results: true`,
    },
  },
  {
    name: 'Apache Iceberg',
    category: 'Lakehouse',
    description: 'Open table format with schema evolution, partition evolution, and hidden partitioning.',
    icon: Database,
    color: 'bg-teal-50 text-teal-600 dark:bg-teal-950/30 dark:text-teal-400',
    status: 'experimental',
    docs: {
      overview: 'The Apache Iceberg connector provides read/write access to Iceberg tables with schema evolution, partition evolution, hidden partitioning, and snapshot isolation. This connector is experimental — API may change.',
      authentication: 'Uses a Hive Metastore or REST catalog for table metadata. Cloud storage credentials (S3/GCS/ADLS) are required from the Vortex vault.',
      capabilities: ['BatchRead', 'BatchWrite', 'Schema Evolution', 'Partition Evolution', 'Snapshot Isolation', 'Time Travel'],
      configExample: `connector:
  type: iceberg
  catalog_type: rest  # or "hive"
  catalog_uri: http://iceberg-catalog:8181
  warehouse: s3://lakehouse/iceberg/
  aws_access_key_vault_key: aws_access_key_id
  aws_secret_key_vault_key: aws_secret_access_key`,
      dagExample: `- task: append_events
  operator: iceberg_append
  config:
    table: catalog.db.events
    source_path: /tmp/new_events.parquet
    partition_spec: "day(event_time)"`,
    },
  },
];

const statusBadge: Record<Connector['status'], string> = {
  available: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400',
  beta: 'bg-amber-100 text-amber-700 dark:bg-amber-500/10 dark:text-amber-400',
  experimental: 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400',
};

export function ConnectorsPage() {
  const categories = [...new Set(CONNECTORS.map((c) => c.category))];
  const [selectedConnector, setSelectedConnector] = useState<Connector | null>(null);

  if (selectedConnector) {
    return (
      <div className="space-y-6">
        <button
          onClick={() => setSelectedConnector(null)}
          className="inline-flex items-center gap-1.5 text-sm font-medium text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
        >
          <ArrowLeft className="h-4 w-4" />
          Back to Connectors
        </button>

        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <div className="flex items-center gap-4">
            <div className={`flex h-12 w-12 shrink-0 items-center justify-center rounded-lg ${selectedConnector.color}`}>
              <selectedConnector.icon className="h-6 w-6" />
            </div>
            <div>
              <h1 className="text-2xl font-bold text-gray-900 dark:text-white">{selectedConnector.name}</h1>
              <div className="mt-1 flex items-center gap-2">
                <span className={`inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium capitalize ${statusBadge[selectedConnector.status]}`}>
                  {selectedConnector.status === 'available' ? <CheckCircle className="h-3 w-3" /> : <AlertCircle className="h-3 w-3" />}
                  {selectedConnector.status}
                </span>
                <span className="text-xs text-gray-500 dark:text-gray-400">{selectedConnector.category}</span>
              </div>
            </div>
          </div>
        </div>

        {/* Overview */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-3">Overview</h2>
          <p className="text-sm text-gray-600 dark:text-gray-300 leading-relaxed">{selectedConnector.docs.overview}</p>
        </div>

        {/* Authentication */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-3">Authentication</h2>
          <p className="text-sm text-gray-600 dark:text-gray-300 leading-relaxed">{selectedConnector.docs.authentication}</p>
        </div>

        {/* Capabilities */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-3">Capabilities</h2>
          <div className="flex flex-wrap gap-2">
            {selectedConnector.docs.capabilities.map((cap) => (
              <span key={cap} className="inline-flex rounded-full bg-gray-100 px-3 py-1 text-xs font-medium text-gray-700 dark:bg-gray-800 dark:text-gray-300">
                {cap}
              </span>
            ))}
          </div>
        </div>

        {/* Configuration Example */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-3">Configuration</h2>
          <pre className="rounded-lg bg-gray-50 p-4 text-xs text-gray-800 overflow-x-auto dark:bg-gray-950 dark:text-gray-200 font-mono leading-relaxed">
            {selectedConnector.docs.configExample}
          </pre>
        </div>

        {/* DAG Usage Example */}
        <div className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm dark:border-gray-800 dark:bg-gray-900">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-3">DAG Usage Example</h2>
          <pre className="rounded-lg bg-gray-50 p-4 text-xs text-gray-800 overflow-x-auto dark:bg-gray-950 dark:text-gray-200 font-mono leading-relaxed">
            {selectedConnector.docs.dagExample}
          </pre>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Connector Ecosystem</h1>
        <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
          Enterprise-grade connectors for cloud data warehouses, object storage, streaming, and lakehouses.
          Configure connectors via the Vortex plugin SDK or operator config in your DAG definitions.
        </p>
      </div>

      {/* Summary bar */}
      <div className="grid grid-cols-3 gap-4">
        {[
          { label: 'Available', count: CONNECTORS.filter((c) => c.status === 'available').length, color: 'text-emerald-600 dark:text-emerald-400' },
          { label: 'Beta', count: CONNECTORS.filter((c) => c.status === 'beta').length, color: 'text-amber-600 dark:text-amber-400' },
          { label: 'Experimental', count: CONNECTORS.filter((c) => c.status === 'experimental').length, color: 'text-gray-600 dark:text-gray-400' },
        ].map((s) => (
          <div key={s.label} className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-800 dark:bg-gray-900 text-center">
            <p className={`text-3xl font-bold ${s.color}`}>{s.count}</p>
            <p className="mt-1 text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400">{s.label}</p>
          </div>
        ))}
      </div>

      {/* Grouped by category */}
      {categories.map((cat) => (
        <div key={cat}>
          <h2 className="mb-3 text-sm font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400">{cat}</h2>
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {CONNECTORS.filter((c) => c.category === cat).map((conn) => (
              <div key={conn.name} className="rounded-xl border border-gray-200 bg-white p-5 shadow-sm dark:border-gray-800 dark:bg-gray-900">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-start gap-3">
                    <div className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg ${conn.color}`}>
                      <conn.icon className="h-5 w-5" />
                    </div>
                    <div>
                      <p className="font-semibold text-gray-900 dark:text-white">{conn.name}</p>
                      <p className="mt-1 text-xs text-gray-500 dark:text-gray-400 leading-relaxed">
                        {conn.description}
                      </p>
                    </div>
                  </div>
                </div>
                <div className="mt-4 flex items-center justify-between">
                  <span className={`inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-medium capitalize ${statusBadge[conn.status]}`}>
                    {conn.status === 'available' ? <CheckCircle className="h-3 w-3" /> : <AlertCircle className="h-3 w-3" />}
                    {conn.status}
                  </span>
                  <button
                    onClick={() => setSelectedConnector(conn)}
                    className="text-xs font-medium text-vortex-600 hover:text-vortex-700 dark:text-vortex-400"
                  >                    View docs →
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
