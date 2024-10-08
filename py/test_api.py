import requests
import uuid
from datetime import datetime

def get_log_info(base_url):
    """
    Retrieves log information from the server.

    Args:
        base_url (str): The base URL of the API server.

    Returns:
        dict: The log information if successful, None otherwise.
    """
    url = f"{base_url}/api/log_info"
    response = requests.get(url)
    if response.status_code == 200:
        return response.json()
    else:
        print(f"Failed to get log info: {response.status_code} - {response.text}")
        return None

def purge_logs(base_url):
    """
    Purges all logs from the server.

    Args:
        base_url (str): The base URL of the API server.

    Returns:
        str: The server's response message if successful, None otherwise.
    """
    url = f"{base_url}/api/purge_logs"
    response = requests.post(url)
    if response.status_code == 200:
        return response.json()
    else:
        print(f"Failed to purge logs: {response.status_code} - {response.text}")
        return None

def insert_log(base_url, log_message):
    """
    Inserts a log message into the server.

    Args:
        base_url (str): The base URL of the API server.
        log_message (dict): The log message to insert.

    Returns:
        str: The server's response message if successful, None otherwise.
    """
    url = f"{base_url}/api/insert_log"
    headers = {'Content-Type': 'application/json'}
    response = requests.post(url, json=log_message, headers=headers)
    if response.status_code == 200:
        return response.json()
    else:
        print(f"Failed to insert log: {response.status_code} - {response.text}")
        return None

def main():
    base_url = 'http://localhost:8000'  # Adjust the port and IP as needed
    
    # Test get_log_info
    print("Testing get_log_info...")
    log_info = get_log_info(base_url)
    if log_info:
        print("Log Info:")
        print(log_info)
    else:
        print("No log info available or failed to retrieve.")

    # Test purge_logs
    print("\nTesting purge_logs...")
    result = purge_logs(base_url)
    if result:
        print("Purge Logs Result:")
        print(result)
    else:
        print("Failed to purge logs.")

    # Test insert_log
    print("\nTesting insert_log...")
    log_message = {
        "level": "INFO",
        "message": "Test log message from Python client.",
        "target": "python_test_client",
        "module_path": "test_module",
        "file": "test_file.py",
        "line": 42,
        "hash": str(uuid.uuid4()),
        "timestamp": datetime.utcnow().isoformat() + 'Z'
    }
    
    result = insert_log(base_url, log_message)
    if result:
        print("Insert Log Result:")
        print(result)
    else:
        print("Failed to insert log.")

    # Verify that the log was inserted by fetching log info again
    print("\nVerifying log insertion by calling get_log_info again...")
    log_info = get_log_info(base_url)
    if log_info:
        print("Updated Log Info:")
        print(log_info)
    else:
        print("Failed to retrieve updated log info.")

if __name__ == '__main__':
    main()
