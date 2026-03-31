# Airflow to Vortex Migration Guide

## Overview
This guide describes how to migrate Airflow DAGs into native Vortex Rust DAG modules using the Vortex CLI transpiler, including AI-assisted agentic conversion for complex Python and dbt logic.

## Prerequisites
- Rust toolchain installed.
- Vortex repository checked out.
- Python DAG files available locally.
- For agentic mode: an API key for OpenAI or Anthropic.

## Run Migration
Use the CLI migrate command:

```bash
vortex-cli migrate ./dags --output-dir ./generated_dags
```

Strict mode fails when unresolved placeholders are produced:

```bash
vortex-cli migrate ./dags --output-dir ./generated_dags --strict
```

Shim fallback mode preserves original Python callable payloads for runtime fallback:

```bash
vortex-cli migrate ./dags --output-dir ./generated_dags --use-shim-fallback
```

Custom report format (JSON or Markdown):

```bash
vortex-cli migrate ./dags --output-dir ./generated_dags --report-format md
```

### Agentic Conversion Mode

Agentic mode uses LLM providers to automatically translate unresolved `PythonOperator` logic and dbt projects into native Rust:

```bash
# OpenAI provider
vortex-cli migrate ./dags --output-dir ./generated_dags --agentic --llm-provider openai --model gpt-4o-mini

# Anthropic provider
vortex-cli migrate ./dags --output-dir ./generated_dags --agentic --llm-provider anthropic --model claude-3-5-sonnet-latest
```

Required environment variables by provider:
- **OpenAI:** `OPENAI_API_KEY` (optional override: `OPENAI_ENDPOINT`)
- **Anthropic:** `ANTHROPIC_API_KEY` (optional override: `ANTHROPIC_ENDPOINT`)

#### How the Python-to-Rust Agent Works

When agentic mode encounters an unresolved `PythonOperator`, it runs an iterative translation loop:

1. **Analyze** — The LLM analyzes the Python callable source and its dependencies.
2. **Plan** — It proposes a Rust equivalent with appropriate crates and types.
3. **Generate** — It produces candidate Rust code.
4. **Validate** — The candidate is compiled (`cargo check`) and checked against lint policy.
5. **Repair** — If validation fails, compilation errors and lint feedback are fed back to the LLM for another attempt.
6. **Accept or reject** — The loop continues until the code passes or the retry budget is exhausted.

**Safety guardrails:**
- Dangerous APIs (filesystem writes, network access, process spawning) are blocked by policy unless explicitly allowed.
- All generated code must have explicit error handling.
- Token/cost telemetry tracks LLM usage per conversion.

#### dbt-to-Rust Pipeline

For dbt projects, agentic mode can convert an entire dbt project into a native Rust pipeline:

1. **Parse manifest** — Loads the dbt `manifest.json` to extract models, sources, macros, and refs.
2. **Expand Jinja SQL** — Renders Jinja templates with deterministic context.
3. **Build dependency graph** — Constructs a DAG of SQL transformations from the manifest.
4. **Map to connectors** — Each node is mapped to the appropriate enterprise connector execution stage.
5. **Generate Rust module** — Produces a Rust orchestration module representing the full dbt DAG.
6. **Validate** — Runs `cargo check` and snapshot tests against expected SQL shapes.

```bash
vortex-cli migrate ./dbt_project --output-dir ./generated_dags --agentic --llm-provider openai --model gpt-4o-mini
```

## Output Artifacts
- One generated Rust module per DAG: `<dag_id>_generated.rs`
- JSON or Markdown summary report: `migration_report.json` / `migration_report.md`

`migration_report.json` contains:
- `dag_id`
- `generated_file`
- `converted_tasks` — Number of tasks successfully converted
- `placeholder_tasks` — Number of tasks requiring manual conversion or agentic translation
- `agentic_conversions` — Number of tasks translated by the LLM agent (when `--agentic` is used)

## Placeholder Handling
Unsupported `PythonOperator` logic is emitted as a strict migration placeholder payload:
- Placeholder text is preserved for future agentic conversion.
- DAG execution can fall back to the existing Python shim path in current runtime.
- In `--strict` mode, any remaining placeholder causes migration to fail.

## Validation Checklist
1. Run `cargo check` after generation.
2. Verify dependency graph parity against source DAG (automated graph-equivalence checks run during migration).
3. Confirm schedules and task IDs match expected names.
4. Review all placeholder tasks for manual conversion or agentic translation.
5. Ensure generated Rust passes syntax validation and strict graph-equivalence checks.
6. For agentic conversions, review generated code for correctness and security before promoting to production.

## Cutover Strategy
1. Generate DAGs and validate topology.
2. Run dry integration in staging.
3. Replace unresolved placeholders incrementally (or use `--agentic` for automated resolution).
4. Promote to production and monitor scheduler/task metrics.
5. Compare latency and resource usage against Airflow baselines using the connector benchmarking harness.
