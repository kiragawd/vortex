"""
XCom helper for VORTEX Python tasks.
Tasks receive XCom functions via the task context (environment variables).

Typical usage inside a VORTEX Python task
------------------------------------------
from vortex.xcom import xcom_push, xcom_pull

# Push a result (reads VORTEX_DAG_ID / VORTEX_TASK_ID / VORTEX_RUN_ID from env)
xcom_push("return_value", {"rows_processed": 42})

# Pull a value written by an upstream task called "extract"
data = xcom_pull("extract", key="return_value")
"""

import os
import json
import urllib.request
import urllib.error


def xcom_push(key: str, value, dag_id: str = None, task_id: str = None, run_id: str = None):
    """Push a value to XCom.

    Auto-reads dag_id / task_id / run_id from environment variables if not provided:
        VORTEX_DAG_ID, VORTEX_TASK_ID, VORTEX_RUN_ID

    Non-string values are JSON-serialised before being stored so that
    ``xcom_pull`` can transparently deserialise them on retrieval.

    Parameters
    ----------
    key:
        Arbitrary label for this value (e.g. ``"return_value"``).
    value:
        Any JSON-serialisable object or a plain string.
    dag_id, task_id, run_id:
        Override the identifiers read from the environment.

    Returns
    -------
    dict
        The JSON response from the VORTEX API.

    Raises
    ------
    ValueError
        If dag_id, task_id, or run_id cannot be determined.
    RuntimeError
        If the API call fails.
    """
    dag_id = dag_id or os.environ.get("VORTEX_DAG_ID", "")
    task_id = task_id or os.environ.get("VORTEX_TASK_ID", "")
    run_id = run_id or os.environ.get("VORTEX_RUN_ID", "")
    api_key = os.environ.get("VORTEX_API_KEY", "vortex_admin_key")
    base_url = os.environ.get("VORTEX_BASE_URL", "http://localhost:3000")

    if not all([dag_id, task_id, run_id]):
        raise ValueError(
            "dag_id, task_id, and run_id are required "
            "(set VORTEX_DAG_ID, VORTEX_TASK_ID, VORTEX_RUN_ID)"
        )

    # Serialise non-string values so the Rust side always stores a string.
    serialised_value = value if isinstance(value, str) else json.dumps(value)

    payload = json.dumps({
        "dag_id": dag_id,
        "task_id": task_id,
        "run_id": run_id,
        "key": key,
        "value": serialised_value,
    }).encode()

    req = urllib.request.Request(
        f"{base_url}/api/xcom/push",
        data=payload,
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {api_key}",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"XCom push failed: {e.read().decode()}") from e


def xcom_pull(
    task_id: str,
    key: str = "return_value",
    dag_id: str = None,
    run_id: str = None,
):
    """Pull a value from XCom (typically from an upstream task).

    Auto-reads dag_id / run_id from environment variables if not provided:
        VORTEX_DAG_ID, VORTEX_RUN_ID

    If the stored value is valid JSON it is automatically deserialised;
    otherwise the raw string is returned.

    Parameters
    ----------
    task_id:
        The task whose XCom value you want to read.
    key:
        The XCom key (default ``"return_value"``).
    dag_id, run_id:
        Override the identifiers read from the environment.

    Returns
    -------
    object or None
        The deserialised value, or ``None`` if not found.

    Raises
    ------
    ValueError
        If dag_id, task_id, or run_id cannot be determined.
    """
    dag_id = dag_id or os.environ.get("VORTEX_DAG_ID", "")
    run_id = run_id or os.environ.get("VORTEX_RUN_ID", "")
    api_key = os.environ.get("VORTEX_API_KEY", "vortex_admin_key")
    base_url = os.environ.get("VORTEX_BASE_URL", "http://localhost:3000")

    if not all([dag_id, task_id, run_id]):
        raise ValueError("dag_id, task_id, and run_id are required")

    url = (
        f"{base_url}/api/xcom/pull"
        f"?dag_id={dag_id}&task_id={task_id}&run_id={run_id}&key={key}"
    )
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {api_key}"})
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read())
            raw = data.get("value")
            if raw is not None:
                try:
                    return json.loads(raw)
                except (json.JSONDecodeError, TypeError):
                    return raw
            return None
    except urllib.error.HTTPError:
        return None


def xcom_pull_all(dag_id: str = None, run_id: str = None):
    """Pull all XCom entries for the current (or specified) run.

    Useful for debugging or generating run summaries.

    Parameters
    ----------
    dag_id, run_id:
        Override the identifiers read from the environment.

    Returns
    -------
    list[dict]
        List of ``{dag_id, task_id, run_id, key, value, timestamp}`` dicts.
        Values are left as raw strings (JSON-deserialise manually if needed).
    """
    dag_id = dag_id or os.environ.get("VORTEX_DAG_ID", "")
    run_id = run_id or os.environ.get("VORTEX_RUN_ID", "")
    api_key = os.environ.get("VORTEX_API_KEY", "vortex_admin_key")
    base_url = os.environ.get("VORTEX_BASE_URL", "http://localhost:3000")

    if not all([dag_id, run_id]):
        raise ValueError("dag_id and run_id are required")

    url = f"{base_url}/api/dags/{dag_id}/runs/{run_id}/xcom"
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {api_key}"})
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError:
        return []
