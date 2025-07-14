use crate::{ExecuteResponse, ExecutionError, generate_vm_id};
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Method, Request, Uri};
use hyper_util::client::legacy::Client;
use hyperlocal::{UnixClientExt, UnixConnector};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::time::timeout;

/// VM Manager for handling Firecracker VM lifecycle
#[allow(dead_code)]
pub struct VMManager {
    vm_id: String,
    socket_path: String,
    process: Option<Child>,
}

impl Default for VMManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VMManager {
    /// Create a new VM manager with unique ID
    pub fn new() -> Self {
        let vm_id = generate_vm_id();
        let socket_path = format!("/tmp/firecracker-{}.socket", vm_id);

        Self {
            vm_id,
            socket_path,
            process: None,
        }
    }

    /// Start the Firecracker process
    pub async fn start_firecracker(&mut self) -> Result<(), ExecutionError> {
        let mut child = tokio::process::Command::new("firecracker")
            .arg("--api-sock")
            .arg(&self.socket_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                ExecutionError::ProcessSpawnError(format!("Failed to start Firecracker: {}", e))
            })?;

        // Wait for the socket to be created
        let socket_wait = timeout(Duration::from_secs(5), async {
            loop {
                if std::path::Path::new(&self.socket_path).exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        match socket_wait.await {
            Ok(_) => {
                self.process = Some(child);
                Ok(())
            }
            Err(_) => {
                let _ = child.kill().await;
                Err(ExecutionError::StartupError(
                    "Socket creation timeout".to_string(),
                ))
            }
        }
    }

    /// Send HTTP request to Firecracker API via Unix socket
    async fn send_api_request(
        &self,
        method: Method,
        path: &str,
        body: Option<&str>,
    ) -> Result<(), ExecutionError> {
        let client: Client<UnixConnector, Full<Bytes>> = Client::unix();

        let uri: Uri = hyperlocal::Uri::new(&self.socket_path, path).into();

        let mut request_builder = Request::builder().method(method.clone()).uri(uri);

        let request = if let Some(json_body) = body {
            request_builder = request_builder.header("content-type", "application/json");
            request_builder
                .body(Full::new(Bytes::from(json_body.to_string())))
                .map_err(|e| {
                    ExecutionError::ApiCommunicationError(format!("Request build failed: {}", e))
                })?
        } else {
            request_builder.body(Full::new(Bytes::new())).map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Request build failed: {}", e))
            })?
        };

        let response = client.request(request).await.map_err(|e| {
            ExecutionError::ApiCommunicationError(format!("API request failed: {}", e))
        })?;

        let status = response.status();

        if !status.is_success() {
            use http_body_util::BodyExt;
            let body_bytes = response
                .collect()
                .await
                .map_err(|e| {
                    ExecutionError::ApiCommunicationError(format!(
                        "Failed to read error response: {}",
                        e
                    ))
                })?
                .to_bytes();

            let error_body = String::from_utf8_lossy(&body_bytes);
            return Err(ExecutionError::ApiCommunicationError(format!(
                "API returned error status: {} for {} {}. Error details: {}",
                status, method, path, error_body
            )));
        }

        Ok(())
    }

    /// Configure the VM via HTTP API
    pub async fn configure_vm(&self) -> Result<(), ExecutionError> {
        // Set machine configuration
        let machine_config = std::fs::read_to_string("fixtures/machine.json").map_err(|e| {
            ExecutionError::ResourceError(format!("Failed to read machine config: {}", e))
        })?;
        let machine_config: serde_json::Value = serde_json::from_str(&machine_config).unwrap();

        self.send_api_request(
            Method::PUT,
            "/machine-config",
            Some(&machine_config.to_string()),
        )
        .await
        .map_err(|e| {
            ExecutionError::ApiCommunicationError(format!("Machine config failed: {}", e))
        })?;

        // Set boot source
        let boot_source = serde_json::json!({
            "kernel_image_path": "./hello-vmlinux.bin",
            "boot_args": "console=ttyS0 reboot=k panic=1 pci=off"
        });

        self.send_api_request(Method::PUT, "/boot-source", Some(&boot_source.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Boot source config failed: {}", e))
            })?;

        // Set root filesystem
        let rootfs = serde_json::json!({
            "drive_id": "rootfs",
            "path_on_host": "./alpine-python.ext4",
            "is_root_device": true,
            "is_read_only": false
        });

        self.send_api_request(Method::PUT, "/drives/rootfs", Some(&rootfs.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Rootfs config failed: {}", e))
            })?;

        // Start the VM
        let start_action = serde_json::json!({
            "action_type": "InstanceStart"
        });

        self.send_api_request(Method::PUT, "/actions", Some(&start_action.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("VM start failed: {}", e))
            })?;

        Ok(())
    }

    /// Execute Python code inside the Firecracker VM
    pub async fn execute_code(&mut self, code: &str) -> Result<ExecuteResponse, ExecutionError> {
        use tokio::time::{Duration, timeout};

        if let Some(ref mut process) = self.process {
            // Wait for VM to boot up completely (give it time to initialize)
            tokio::time::sleep(Duration::from_secs(3)).await;

            // Get stdin, stdout, stderr handles
            let stdin = process.stdin.as_mut().ok_or_else(|| {
                ExecutionError::ProcessSpawnError("Failed to get VM stdin".to_string())
            })?;

            let stdout = process.stdout.as_mut().ok_or_else(|| {
                ExecutionError::ProcessSpawnError("Failed to get VM stdout".to_string())
            })?;

            let stderr = process.stderr.as_mut().ok_or_else(|| {
                ExecutionError::ProcessSpawnError("Failed to get VM stderr".to_string())
            })?;

            // Prepare the command to execute Python code
            // We need to escape quotes properly for shell execution
            let escaped_code = code.replace("'", "'\"'\"'");
            let command = format!(
                "python3 -c '{}'\necho \"__EXECUTION_COMPLETE__\"\n",
                escaped_code
            );

            // Send the command to VM stdin
            if let Err(e) = stdin.write_all(command.as_bytes()).await {
                return Err(ExecutionError::ProcessSpawnError(format!(
                    "Failed to send code to VM: {}",
                    e
                )));
            }

            // Force flush the input
            if let Err(e) = stdin.flush().await {
                return Err(ExecutionError::ProcessSpawnError(format!(
                    "Failed to flush input to VM: {}",
                    e
                )));
            }

            // Read output with timeout
            let read_timeout = Duration::from_secs(10);

            let mut stdout_buffer = Vec::new();
            let mut stderr_buffer = Vec::new();
            let mut temp_buffer = [0u8; 1024];

            // Read stdout until we see the completion marker or timeout
            let stdout_result: Result<Result<(), ExecutionError>, tokio::time::error::Elapsed> =
                timeout(read_timeout, async {
                    loop {
                        match stdout.read(&mut temp_buffer).await {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                stdout_buffer.extend_from_slice(&temp_buffer[..n]);
                                let output_str = String::from_utf8_lossy(&stdout_buffer);

                                // Check if we've received the completion marker
                                if output_str.contains("__EXECUTION_COMPLETE__") {
                                    break;
                                }
                            }
                            Err(e) => {
                                return Err(ExecutionError::ProcessSpawnError(format!(
                                    "Failed to read VM stdout: {}",
                                    e
                                )));
                            }
                        }
                    }
                    Ok(())
                })
                .await;

            // Read stderr with a shorter timeout since it should be quick
            let _stderr_result: Result<Result<(), ExecutionError>, tokio::time::error::Elapsed> =
                timeout(Duration::from_secs(2), async {
                    loop {
                        match stderr.read(&mut temp_buffer).await {
                            Ok(0) => break, // EOF
                            Ok(n) => {
                                stderr_buffer.extend_from_slice(&temp_buffer[..n]);
                            }
                            Err(_) => break, // Error reading, assume no more stderr
                        }

                        // Don't read stderr forever
                        if stderr_buffer.len() > 10_000 {
                            break;
                        }
                    }
                    Ok(())
                })
                .await;

            // Check for timeout
            if stdout_result.is_err() {
                return Err(ExecutionError::TimeoutError);
            }

            // Process the output
            let stdout_str = String::from_utf8_lossy(&stdout_buffer);
            let stderr_str = String::from_utf8_lossy(&stderr_buffer);

            // Remove the completion marker and any VM boot messages
            let cleaned_stdout = stdout_str
                .replace("__EXECUTION_COMPLETE__", "")
                .lines()
                .filter(|line| {
                    // Filter out common VM boot messages and keep only Python output
                    !line.contains("login:")
                        && !line.contains("Welcome to")
                        && !line.contains("Alpine Linux")
                        && !line.contains("localhost")
                        && !line.contains("#")
                        && !line.trim().is_empty()
                })
                .collect::<Vec<_>>()
                .join("\n");

            let final_stdout = if cleaned_stdout.trim().is_empty() {
                String::new()
            } else {
                cleaned_stdout.trim().to_string() + "\n"
            };

            let final_stderr = stderr_str.trim().to_string();
            let success = final_stderr.is_empty();

            Ok(ExecuteResponse {
                stdout: final_stdout,
                stderr: final_stderr,
                success,
            })
        } else {
            Err(ExecutionError::ProcessSpawnError(
                "No VM process available".to_string(),
            ))
        }
    }

    /// Clean up VM resources
    pub async fn cleanup(mut self) -> Result<(), ExecutionError> {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill().await;
            let _ = process.wait().await;
        }

        // Remove socket file
        if std::path::Path::new(&self.socket_path).exists() {
            std::fs::remove_file(&self.socket_path).map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to remove socket: {}", e))
            })?;
        }

        Ok(())
    }
}

/// Execute Python code in a fresh Firecracker microVM
pub async fn run_in_vm(code: &str) -> Result<ExecuteResponse, ExecutionError> {
    let mut vm_manager = VMManager::new();

    // Start Firecracker process
    vm_manager.start_firecracker().await?;

    // Configure the VM
    vm_manager.configure_vm().await?;

    // Execute the code
    let result = vm_manager.execute_code(code).await;

    // Clean up resources
    let _ = vm_manager.cleanup().await;

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_manager_creation() {
        let vm_manager = VMManager::new();
        assert!(!vm_manager.vm_id.is_empty());
        assert!(vm_manager.socket_path.contains("/tmp/firecracker-"));
        assert!(vm_manager.socket_path.ends_with(".socket"));
    }

    #[tokio::test]
    async fn test_vm_manager_cleanup() {
        let socket_path = "/tmp/test-socket.socket";

        // Create a test socket file
        std::fs::File::create(socket_path).unwrap();
        assert!(std::path::Path::new(socket_path).exists());

        // Create VMManager with test socket
        let mut vm_manager = VMManager::new();
        vm_manager.socket_path = socket_path.to_string();

        // Cleanup should remove the socket
        vm_manager.cleanup().await.unwrap();
        assert!(!std::path::Path::new(socket_path).exists());
    }

    #[test]
    fn test_python_command_escaping() {
        let code = "print('hello world')";
        let expected = "python3 -c 'print(\\'hello world\\')'\n";
        let actual = format!("python3 -c '{}'\n", code.replace("'", "\\'"));
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_complex_python_code_escaping() {
        let code = "x = 'test'; print(f'Value: {x}')";
        let expected = "python3 -c 'x = \\'test\\'; print(f\\'Value: {x}\\')'\n";
        let actual = format!("python3 -c '{}'\n", code.replace("'", "\\'"));
        assert_eq!(actual, expected);
    }
}
