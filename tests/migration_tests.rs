/// Legacy Migration Tests
/// Tests TWS parsing, Autosys JIL parsing, conversion, and code generation

#[cfg(test)]
mod migration_tests {
    use vortex::migration::*;

    #[test]
    fn test_tws_parser_basic_jobs() {
        let input = r#"
SCHEDULE my_schedule
JOBS
job_a DOCOMMAND "/opt/scripts/extract.sh"
  FOLLOWS job_b
job_b DOCOMMAND "/opt/scripts/load.sh"
END
"#;
        let result = parse_tws_definition(input, "test.tws");
        assert!(result.is_ok(), "TWS parse failed: {:?}", result.err());
        let jobs = result.unwrap();
        assert!(!jobs.is_empty(), "Should extract at least one job");
        assert!(jobs.iter().any(|j| j.source == SourceScheduler::Tws));
    }

    #[test]
    fn test_tws_parser_empty_returns_empty() {
        let input = "# Just comments\n\n";
        let result = parse_tws_definition(input, "empty.tws");
        assert!(result.is_ok());
        let jobs = result.unwrap();
        assert!(jobs.is_empty());
    }

    #[test]
    fn test_autosys_jil_single_job() {
        let input = r#"
insert_job: etl_extract
job_type: c
command: /opt/etl/extract.sh
machine: prod-server-01
owner: etl_user
condition: s(data_check)
date_conditions: 1
days_of_week: mo,tu,we,th,fr
"#;
        let result = parse_autosys_jil(input, "test.jil");
        assert!(result.is_ok(), "JIL parse failed: {:?}", result.err());
        let jobs = result.unwrap();
        assert!(!jobs.is_empty());
        let job = &jobs[0];
        assert_eq!(job.job_name, "etl_extract");
        assert!(job.dependencies.contains(&"data_check".to_string()));
        assert_eq!(job.source, SourceScheduler::Autosys);
    }

    #[test]
    fn test_autosys_jil_multiple_jobs() {
        let input = r#"
insert_job: job_a
job_type: c
command: /bin/true

insert_job: job_b
job_type: c
command: /bin/true
condition: s(job_a)

insert_job: job_c
job_type: b
"#;
        let result = parse_autosys_jil(input, "multi.jil");
        assert!(result.is_ok());
        let jobs = result.unwrap();
        assert_eq!(jobs.len(), 3);
    }

    #[test]
    fn test_autosys_jil_empty() {
        let input = "# Empty JIL file\n";
        let result = parse_autosys_jil(input, "empty.jil");
        assert!(result.is_ok());
    }

    #[test]
    fn test_convert_to_vortex_dag() {
        let jobs = vec![
            MigrationJob {
                source: SourceScheduler::Tws,
                job_name: "extract".to_string(),
                job_type: "command".to_string(),
                command: Some("/opt/extract.sh".to_string()),
                schedule: None,
                dependencies: vec![],
                conditions: vec![],
                resources: std::collections::HashMap::new(),
                notifications: vec![],
                properties: std::collections::HashMap::new(),
            },
            MigrationJob {
                source: SourceScheduler::Tws,
                job_name: "transform".to_string(),
                job_type: "command".to_string(),
                command: Some("/opt/transform.sh".to_string()),
                schedule: None,
                dependencies: vec!["extract".to_string()],
                conditions: vec![],
                resources: std::collections::HashMap::new(),
                notifications: vec![],
                properties: std::collections::HashMap::new(),
            },
        ];
        let result = convert_to_vortex_dag(&jobs, "test_pipeline");
        assert_eq!(result.source, SourceScheduler::Tws);
        assert_eq!(result.jobs_parsed, 2);
        assert_eq!(result.jobs_converted, 2);
        assert_eq!(result.vortex_dag.dag_id, "test_pipeline");
        assert_eq!(result.vortex_dag.tasks.len(), 2);
    }

    #[test]
    fn test_convert_warns_on_invalid_dependency() {
        let jobs = vec![MigrationJob {
            source: SourceScheduler::Autosys,
            job_name: "orphan_job".to_string(),
            job_type: "command".to_string(),
            command: Some("/bin/true".to_string()),
            schedule: None,
            dependencies: vec!["nonexistent_parent".to_string()],
            conditions: vec![],
            resources: std::collections::HashMap::new(),
            notifications: vec![],
            properties: std::collections::HashMap::new(),
        }];
        let result = convert_to_vortex_dag(&jobs, "orphan_dag");
        // The orphan dependency should be filtered out and warned about
        assert!(!result.warnings.is_empty() || result.vortex_dag.tasks[0].dependencies.is_empty(),
                "Should warn about or remove invalid dependency");
    }

    #[test]
    fn test_generate_rust_code() {
        let dag = VortexDagDef {
            dag_id: "etl_pipeline".to_string(),
            description: "Test pipeline".to_string(),
            schedule: Some("0 8 * * *".to_string()),
            tasks: vec![VortexTaskDef {
                task_id: "extract".to_string(),
                operator: "ShellOperator".to_string(),
                config: serde_json::json!({"command": "/opt/extract.sh"}),
                dependencies: vec![],
                retries: 3,
                retry_delay_secs: 60,
            }],
            default_args: std::collections::HashMap::new(),
        };
        let code = generate_rust_dag_code(&dag);
        assert!(code.contains("etl_pipeline"), "Should contain dag_id");
        assert!(code.contains("extract"), "Should contain task ID");
        assert!(code.contains("create_dag"), "Should contain fn name");
    }

    #[test]
    fn test_generate_python_code() {
        let dag = VortexDagDef {
            dag_id: "py_pipeline".to_string(),
            description: "Python test".to_string(),
            schedule: None,
            tasks: vec![VortexTaskDef {
                task_id: "step_one".to_string(),
                operator: "ShellOperator".to_string(),
                config: serde_json::json!({"command": "echo hello"}),
                dependencies: vec![],
                retries: 2,
                retry_delay_secs: 30,
            }],
            default_args: std::collections::HashMap::new(),
        };
        let code = generate_python_dag_code(&dag);
        assert!(code.contains("py_pipeline"), "Should contain dag_id");
        assert!(code.contains("step_one"), "Should contain task_id");
        assert!(code.contains("from vortex"), "Should import vortex");
    }

    #[test]
    fn test_generate_migration_report() {
        let results = vec![MigrationResult {
            source: SourceScheduler::Tws,
            source_file: "test.tws".to_string(),
            jobs_parsed: 5,
            jobs_converted: 4,
            warnings: vec!["Some warning".to_string()],
            errors: vec![],
            vortex_dag: VortexDagDef {
                dag_id: "report_test".to_string(),
                description: "test".to_string(),
                schedule: None,
                tasks: vec![],
                default_args: std::collections::HashMap::new(),
            },
        }];
        let report = generate_migration_report(&results);
        assert!(report.contains("Migration Report"));
        assert!(report.contains("report_test") || report.contains("test.tws"));
    }
}
