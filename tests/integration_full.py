import requests
import time
import os
import sys

BASE_URL = "http://localhost:3000/api"
API_KEY = "vortex_admin_key" # Will try to fetch or default
HEADERS = {"Authorization": f"Bearer {API_KEY}"}

def wait_for_server(timeout=30):
    start_time = time.time()
    while time.time() - start_time < timeout:
        try:
            response = requests.get(f"{BASE_URL}/dags", headers=HEADERS)
            if response.status_code == 200:
                print("Server is up and running.")
                return True
        except requests.exceptions.ConnectionError:
            pass
        time.sleep(1)
    return False

def test_full_pipeline():
    print("\n--- Testing Full Pipeline ---")
    
    # 1. Create Secret
    print("Creating secret...")
    secret_data = {"key": "MY_TEST_SECRET", "value": "top_secret_value"}
    resp = requests.post(f"{BASE_URL}/secrets", json=secret_data, headers=HEADERS)
    if resp.status_code != 200:
        print(f"Failed to create secret: {resp.status_code} {resp.text}")
    assert resp.status_code == 200
    
    # 2. Upload DAG
    print("Uploading complex DAG...")
    dag_path = "/Users/ashwin/vortex/tests/fixtures/integration_test_dag.py"
    with open(dag_path, "rb") as f:
        files = {"file": ("integration_test_dag.py", f)}
        resp = requests.post(f"{BASE_URL}/dags/upload", files=files, headers=HEADERS)
    
    assert resp.status_code == 200
    dag_info = resp.json()
    dag_id = dag_info["dag_id"]
    print(f"DAG uploaded: {dag_id}")
    
    # 3. Validate
    print("Validating DAG registry...")
    resp = requests.get(f"{BASE_URL}/dags", headers=HEADERS)
    dags = resp.json()
    assert any(d["id"] == dag_id for d in dags)
    
    # 4. Trigger
    print(f"Triggering DAG run for {dag_id}...")
    resp = requests.post(f"{BASE_URL}/dags/{dag_id}/trigger", headers=HEADERS)
    if resp.status_code != 200:
        print(f"Trigger failed: {resp.status_code} {resp.text}")
    assert resp.status_code == 200
    
    # 5. Monitor
    print("Monitoring DAG run status...")
    max_retries = 60
    run_completed = False
    for i in range(max_retries):
        # We need task instances, not DAG runs (VORTEX CLI/UI logic differs)
        resp = requests.get(f"{BASE_URL}/dags/{dag_id}/tasks", headers=HEADERS)
        data = resp.json()
        
        instances = data.get("instances", [])
        
        if instances:
            states = [ti["state"] for ti in instances]
            print(f"Current task states: {states}")
            
            if all(s == "Success" for s in states) and len(states) >= 3:
                run_completed = True
                print("All tasks succeeded!")
                
                # Check logs for Task 1 (Bash)
                t1_ti = next(ti for ti in instances if ti["task_id"] == "task_1")
                log_resp = requests.get(f"{BASE_URL}/tasks/{t1_ti['id']}/logs", headers=HEADERS)
                logs = log_resp.json().get("stdout", "") + log_resp.json().get("stderr", "")
                assert "Bash task 1 executed" in logs
                print("Task 1 logs verified.")
                
                # Check logs for Task 2 (Python)
                t2_ti = next(ti for ti in instances if ti["task_id"] == "task_2")
                log_resp = requests.get(f"{BASE_URL}/tasks/{t2_ti['id']}/logs", headers=HEADERS)
                logs = log_resp.json().get("stdout", "") + log_resp.json().get("stderr", "")
                assert "Python task 2 executed successfully" in logs
                print("Task 2 logs verified.")

                # Check logs for Task 3 (Secret)
                t3_ti = next(ti for ti in instances if ti["task_id"] == "task_3")
                log_resp = requests.get(f"{BASE_URL}/tasks/{t3_ti['id']}/logs", headers=HEADERS)
                logs = log_resp.json().get("stdout", "") + log_resp.json().get("stderr", "")
                assert "Secret verified successfully" in logs
                print("Task 3 (Secret Injection) verified.")
                
                break
            
            if any(s == "Failed" for s in states):
                # Print logs of failed task for debugging
                for ti in instances:
                    if ti["state"] == "Failed":
                        log_resp = requests.get(f"{BASE_URL}/tasks/{ti['id']}/logs", headers=HEADERS)
                        logs = log_resp.json().get("stdout", "") + log_resp.json().get("stderr", "")
                        print(f"Task {ti['task_id']} failed with logs:\n{logs}")
                raise RuntimeError("Integration DAG failed")
                
        time.sleep(2)
    
    assert run_completed, "DAG run timed out"
    print("Full pipeline test PASSED.")

def test_error_handling_and_retries():
    print("\n--- Testing Error Handling & Retries ---")
    
    # 1. Upload Failing DAG
    print("Uploading retry test DAG...")
    dag_path = "/Users/ashwin/vortex/tests/fixtures/retry_test_dag.py"
    with open(dag_path, "rb") as f:
        files = {"file": ("retry_test_dag.py", f)}
        resp = requests.post(f"{BASE_URL}/dags/upload", files=files, headers=HEADERS)
    
    assert resp.status_code == 200
    dag_id = resp.json()["dag_id"]
    
    # 2. Trigger
    print(f"Triggering retry test for {dag_id}...")
    requests.post(f"{BASE_URL}/dags/{dag_id}/trigger", headers=HEADERS)
    
    # 3. Monitor for retries
    print("Monitoring retries (expecting 3 attempts total)...")
    max_retries = 60
    final_failed = False
    for i in range(max_retries):
        resp = requests.get(f"{BASE_URL}/dags/{dag_id}/tasks", headers=HEADERS)
        data = resp.json()
        instances = data.get("instances", [])
        
        if instances:
            ti = instances[0]
            print(f"Task state: {ti['state']}, Retry count: {ti['retry_count']}")
            
            if ti["state"] == "Failed" and ti["retry_count"] >= 2:
                final_failed = True
                # Verify logs capture traceback
                log_resp = requests.get(f"{BASE_URL}/tasks/{ti['id']}/logs", headers=HEADERS)
                logs = log_resp.json().get("stdout", "") + log_resp.json().get("stderr", "")
                assert "Intentional failure for retry test" in logs
                print("Retries verified in logs.")
                break
        
        time.sleep(2)
        
    assert final_failed, "Retry test did not reach final failed state with expected retries"
    print("Error handling and retries test PASSED.")

if __name__ == "__main__":
    # Get API key from env or db if possible
    if len(sys.argv) > 1:
        API_KEY = sys.argv[1]
        HEADERS["Authorization"] = f"Bearer {API_KEY}"

    if not wait_for_server():
        print("Error: VORTEX server not reachable.")
        sys.exit(1)
        
    try:
        test_full_pipeline()
        test_error_handling_and_retries()
        print("\n✅ ALL INTEGRATION TESTS PASSED!")
    except Exception as e:
        import traceback
        traceback.print_exc()
        print(f"\n❌ TEST FAILED: {e}")
        sys.exit(1)
