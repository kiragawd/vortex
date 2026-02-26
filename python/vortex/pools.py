"""
Pool information helper for VORTEX Python tasks.
Allows tasks to check pool availability programmatically.
"""
import os
import json
import urllib.request
import urllib.error


def get_pool_info(pool_name: str = "default"):
    """Get current pool usage information.

    Returns a dict with keys: name, slots, description, occupied_slots.
    Returns None if the pool does not exist or the request fails.

    Environment variables:
        VORTEX_API_KEY   — API key for authentication (default: vortex_admin_key)
        VORTEX_BASE_URL  — Base URL of the VORTEX API server (default: http://localhost:3000)
    """
    api_key = os.environ.get("VORTEX_API_KEY", "vortex_admin_key")
    base_url = os.environ.get("VORTEX_BASE_URL", "http://localhost:3000")

    url = f"{base_url}/api/pools/{pool_name}"
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {api_key}"})
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError:
        return None


def list_pools():
    """List all pools and their usage.

    Returns a list of dicts, each with keys: name, slots, description, occupied_slots.
    Returns an empty list if the request fails.

    Environment variables:
        VORTEX_API_KEY   — API key for authentication (default: vortex_admin_key)
        VORTEX_BASE_URL  — Base URL of the VORTEX API server (default: http://localhost:3000)
    """
    api_key = os.environ.get("VORTEX_API_KEY", "vortex_admin_key")
    base_url = os.environ.get("VORTEX_BASE_URL", "http://localhost:3000")

    url = f"{base_url}/api/pools"
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {api_key}"})
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError:
        return []


def pool_has_capacity(pool_name: str = "default") -> bool:
    """Convenience helper: returns True if the pool has at least one free slot.

    Useful for tasks that want to self-gate before doing expensive work.
    """
    info = get_pool_info(pool_name)
    if info is None:
        # Can't confirm — assume capacity is available to avoid deadlock.
        return True
    return info.get("occupied_slots", 0) < info.get("slots", 128)


def free_slots(pool_name: str = "default") -> int:
    """Return the number of free slots in the named pool, or -1 on error."""
    info = get_pool_info(pool_name)
    if info is None:
        return -1
    return max(0, info.get("slots", 0) - info.get("occupied_slots", 0))
