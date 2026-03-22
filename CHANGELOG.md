# Changelog

## [Unreleased]

### Added
- Static Airflow parser module for DAG/task/dependency extraction.
- DAG code generator and migration report writer.
- Enterprise connector abstraction and connector registry.
- Initial connector implementations for Postgres, Snowflake, Databricks, dbt, MySQL, and MS SQL.
- Agentic migration foundation (LLM provider interface, Python-to-Rust loop, dbt manifest conversion).
- CLI `migrate` command for Airflow-to-Rust conversion.
- Migration and connector API documentation.

## [0.6.0] - Existing baseline
- Existing scheduler, executor, web API, PostgreSQL backend, and Python compatibility layers.
