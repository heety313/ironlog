import logging
import socket
import threading
import json
from datetime import datetime
import time
import queue

class TcpLogger(logging.Handler):
    def __init__(self, server_addr, hash_value, reconnect_interval=5):
        super().__init__()
        self.server_addr = server_addr
        self.hash = hash_value
        self.reconnect_interval = reconnect_interval
        self.sock = None
        self.queue = queue.Queue()
        self.thread = threading.Thread(target=self._background_sender, daemon=True)
        self.running = True
        self.connect()
        self.thread.start()

    def connect(self):
        while self.running:
            try:
                self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                self.sock.settimeout(5)  # Set a timeout for socket operations
                self.sock.connect(self.server_addr)
                print(f"Connected to server at {self.server_addr}")
                return True
            except socket.error as e:
                print(f"Failed to connect: {e}. Retrying in {self.reconnect_interval} seconds...")
                time.sleep(self.reconnect_interval)
        return False

    def emit(self, record):
        log_message = {
            'timestamp': datetime.utcnow().isoformat() + 'Z',
            'level': 'WARN' if record.levelname == 'WARNING' else record.levelname,
            'message': record.getMessage(),
            'target': record.name,
            'module_path': record.module,
            'file': record.pathname,
            'line': record.lineno,
            'hash': self.hash
        }
        self.queue.put(log_message)

    def _background_sender(self):
        while self.running:
            try:
                log_message = self.queue.get(timeout=1)
                json_message = json.dumps(log_message)
                print(f"Sending log: {json_message}")  # Debug print
                try:
                    self.sock.sendall((json_message + '\n').encode('utf-8'))
                    print("Log sent successfully")  # Debug print
                except socket.error as e:
                    print(f"Socket error while sending: {e}. Attempting to reconnect...")
                    if self.connect():
                        # Retry sending the message
                        self.sock.sendall((json_message + '\n').encode('utf-8'))
                        print("Log sent successfully after reconnection")  # Debug print
                    else:
                        print("Failed to reconnect. Log message lost.")
            except queue.Empty:
                continue  # No logs to send, continue waiting
            except Exception as e:
                print(f"Error in background sender: {e}")

    def close(self):
        self.running = False
        self.thread.join(timeout=5)  # Wait for the background thread to finish
        if self.sock:
            self.sock.close()
        super().close()

if __name__ == '__main__':
    import logging

    # Initialize the logger with server address and hash
    logger = logging.getLogger()
    logger.setLevel(logging.DEBUG)

    # Create and add the TCP logger handler
    tcp_handler = TcpLogger(('127.0.0.1', 5000), 'helo')
    logger.addHandler(tcp_handler)

    print("Starting to send logs...")
    for i in range(10):  # 10 iterations for more logs
        print(f"Sending log batch {i+1}")
        logging.info(f'Application started - iteration {i+1}')
        logging.warning(f'This is a warning - iteration {i+1}')
        logging.error(f'An error occurred - iteration {i+1}')
        logging.debug(f'A debug message - iteration {i+1}')
        time.sleep(0.1)  # Small delay between log messages

    print("Finished sending logs. Waiting for 5 seconds before exiting...")
    time.sleep(5)  # Allow more time for logs to be sent before the program exits

    # Close the TCP handler explicitly
    tcp_handler.close()
    print("TCP handler closed. Exiting program.")