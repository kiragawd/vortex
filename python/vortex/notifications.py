"""
Notification/callback helper for VORTEX Python tasks.
Configure DAG-level callbacks via the REST API.
"""
import os
import json
import urllib.request
import urllib.error


def set_dag_callbacks(dag_id: str, config: dict):
    """Set callback configuration for a DAG.

    config example::

        {
            "on_success": [{"Webhook": {"url": "https://hooks.slack.com/...", "headers": null}}],
            "on_failure": [
                {"Slack": {"webhook_url": "https://hooks.slack.com/...", "channel": "#alerts"}},
                {"Webhook": {"url": "https://my-pagerduty.com/webhook"}}
            ]
        }

    Supported target shapes
    -----------------------
    Webhook::

        {"Webhook": {"url": "https://...", "headers": {"X-Token": "abc"}}}

    Slack::

        {"Slack": {"webhook_url": "https://hooks.slack.com/...", "channel": "#ops"}}

    Email (best-effort, requires sendmail/curl on the server)::

        {
            "Email": {
                "smtp_host": "smtp.gmail.com",
                "smtp_port": 587,
                "from": "vortex@example.com",
                "to": ["oncall@example.com"],
                "username": "vortex@example.com",
                "password": "secret"
            }
        }

    Returns
    -------
    dict
        Parsed JSON response from the VORTEX API.

    Raises
    ------
    RuntimeError
        If the API returns a non-2xx response.
    """
    api_key = os.environ.get("VORTEX_API_KEY", "vortex_admin_key")
    base_url = os.environ.get("VORTEX_BASE_URL", "http://localhost:3000")

    payload = json.dumps({"dag_id": dag_id, "config": config}).encode()
    req = urllib.request.Request(
        f"{base_url}/api/dags/{dag_id}/callbacks",
        data=payload,
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {api_key}",
        },
        method="PUT",
    )
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"Failed to set callbacks: {e.read().decode()}") from e


def get_dag_callbacks(dag_id: str):
    """Get callback configuration for a DAG.

    Parameters
    ----------
    dag_id : str
        The DAG identifier.

    Returns
    -------
    dict or None
        Parsed callback config, or ``None`` if no config is found / an error
        occurs.
    """
    api_key = os.environ.get("VORTEX_API_KEY", "vortex_admin_key")
    base_url = os.environ.get("VORTEX_BASE_URL", "http://localhost:3000")

    url = f"{base_url}/api/dags/{dag_id}/callbacks"
    req = urllib.request.Request(
        url,
        headers={"Authorization": f"Bearer {api_key}"},
    )
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError:
        return None


def delete_dag_callbacks(dag_id: str):
    """Remove all callbacks for a DAG.

    Parameters
    ----------
    dag_id : str
        The DAG identifier.

    Returns
    -------
    dict or None
        Parsed API response, or ``None`` on error.
    """
    api_key = os.environ.get("VORTEX_API_KEY", "vortex_admin_key")
    base_url = os.environ.get("VORTEX_BASE_URL", "http://localhost:3000")

    req = urllib.request.Request(
        f"{base_url}/api/dags/{dag_id}/callbacks",
        headers={"Authorization": f"Bearer {api_key}"},
        method="DELETE",
    )
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError:
        return None
