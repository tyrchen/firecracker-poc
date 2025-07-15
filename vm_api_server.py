#!/usr/bin/env python3
"""
VM API Server - runs inside the Firecracker VM to execute Python code
"""

import json
import sys
import subprocess
import traceback
import tempfile
import os
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse, parse_qs
import threading
import time


class CodeExecutionHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path == "/execute":
            self.handle_execute()
        elif self.path == "/shutdown":
            self.handle_shutdown()
        else:
            self.send_error(404, "Not Found")

    def do_GET(self):
        if self.path == "/health":
            self.handle_health()
        else:
            self.send_error(404, "Not Found")

    def handle_health(self):
        """Health check endpoint"""
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        response = {"status": "healthy", "message": "VM API server is running"}
        self.wfile.write(json.dumps(response).encode())

    def handle_execute(self):
        """Execute Python code"""
        try:
            content_length = int(self.headers["Content-Length"])
            post_data = self.rfile.read(content_length)
            request_data = json.loads(post_data.decode("utf-8"))

            if "code" not in request_data:
                self.send_error(400, "Missing 'code' field")
                return

            code = request_data["code"]

            # Execute the code
            result = self.execute_python_code(code)

            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(result).encode())

        except json.JSONDecodeError:
            self.send_error(400, "Invalid JSON")
        except Exception as e:
            self.send_error(500, f"Internal server error: {str(e)}")

    def handle_shutdown(self):
        """Shutdown the VM"""
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        response = {"status": "shutting_down", "message": "VM is shutting down"}
        self.wfile.write(json.dumps(response).encode())

        # Shutdown the VM after a short delay
        def shutdown_vm():
            time.sleep(1)
            os.system("reboot -f")

        threading.Thread(target=shutdown_vm, daemon=True).start()

    def execute_python_code(self, code):
        """Execute Python code and return the result"""
        try:
            # First, try direct execution without subprocess (safer in restricted environments)
            return self.execute_code_directly(code)
        except Exception as direct_error:
            print(f"Direct execution failed: {direct_error}")
            # Fallback to subprocess method
            return self.execute_code_subprocess(code)

    def execute_code_directly(self, code):
        """Execute Python code directly in the current process"""
        import io
        import contextlib
        import sys

        # Capture stdout and stderr
        stdout_capture = io.StringIO()
        stderr_capture = io.StringIO()

        try:
            # Redirect stdout and stderr
            with contextlib.redirect_stdout(stdout_capture), contextlib.redirect_stderr(
                stderr_capture
            ):
                # Create a new namespace for execution
                exec_globals = {"__name__": "__main__", "__builtins__": __builtins__}
                exec_locals = {}

                # Execute the code
                exec(code, exec_globals, exec_locals)

            return {
                "stdout": stdout_capture.getvalue(),
                "stderr": stderr_capture.getvalue(),
                "exit_code": 0,
                "success": True,
            }

        except Exception as e:
            return {
                "stdout": stdout_capture.getvalue(),
                "stderr": stderr_capture.getvalue() + f"\nExecution error: {str(e)}",
                "exit_code": 1,
                "success": False,
            }

    def execute_code_subprocess(self, code):
        """Execute Python code in a subprocess (fallback method)"""
        try:
            # Ensure /tmp directory exists and is writable
            import os

            os.makedirs("/tmp", exist_ok=True)
            os.chmod("/tmp", 0o1777)  # Set proper permissions for /tmp

            # Create a temporary file with the code
            try:
                with tempfile.NamedTemporaryFile(
                    mode="w", suffix=".py", delete=False, dir="/tmp"
                ) as f:
                    f.write(code)
                    temp_file = f.name

                # Ensure the temp file is executable
                os.chmod(temp_file, 0o644)
            except Exception as temp_error:
                print(f"Failed to create temp file: {temp_error}")
                return {
                    "stdout": "",
                    "stderr": f"Failed to create temporary file: {str(temp_error)}",
                    "exit_code": 1,
                    "success": False,
                }

            # Debug: Print file info
            print(f"Created temp file: {temp_file}")
            print(f"File exists: {os.path.exists(temp_file)}")
            print(
                f"File size: {os.path.getsize(temp_file) if os.path.exists(temp_file) else 'N/A'}"
            )
            print(f"Python executable: {sys.executable}")

            # Execute the Python code
            result = subprocess.run(
                [sys.executable, temp_file],
                capture_output=True,
                text=True,
                timeout=30,  # 30 second timeout
            )

            # Clean up
            os.unlink(temp_file)

            return {
                "stdout": result.stdout,
                "stderr": result.stderr,
                "exit_code": result.returncode,
                "success": result.returncode == 0,
            }

        except subprocess.TimeoutExpired:
            if "temp_file" in locals():
                os.unlink(temp_file)
            return {
                "stdout": "",
                "stderr": "Code execution timed out (30 seconds)",
                "exit_code": 1,
                "success": False,
            }
        except Exception as e:
            if "temp_file" in locals():
                os.unlink(temp_file)
            return {
                "stdout": "",
                "stderr": f"Execution error: {str(e)}\n{traceback.format_exc()}",
                "exit_code": 1,
                "success": False,
            }

    def log_message(self, format, *args):
        """Override to reduce logging noise"""
        pass


def main():
    # Start the HTTP server
    server_address = ("0.0.0.0", 8080)
    httpd = HTTPServer(server_address, CodeExecutionHandler)

    print(f"VM API Server starting on {server_address[0]}:{server_address[1]}")
    print("Ready to receive code execution requests")

    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nShutting down server...")
        httpd.shutdown()


if __name__ == "__main__":
    main()
