use crate::{ExecuteResponse, ExecutionError, create_success_response, generate_vm_id};
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

    /// Configure the VM via HTTP API
    pub async fn configure_vm(&self) -> Result<(), ExecutionError> {
        let client = reqwest::Client::new();
        let base_url = format!("http://unix/{}", self.socket_path);

        // Set machine configuration
        let machine_config = serde_json::json!({
            "vcpu_count": 1,
            "mem_size_mib": 128,
            "ht_enabled": false
        });

        client
            .put(format!("{}/machine-config", base_url))
            .json(&machine_config)
            .send()
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Machine config failed: {}", e))
            })?;

        // Set boot source
        let boot_source = serde_json::json!({
            "kernel_image_path": "./hello-vmlinux.bin",
            "boot_args": "console=ttyS0 reboot=k panic=1 pci=off"
        });

        client
            .put(format!("{}/boot-source", base_url))
            .json(&boot_source)
            .send()
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

        client
            .put(format!("{}/drives/rootfs", base_url))
            .json(&rootfs)
            .send()
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Rootfs config failed: {}", e))
            })?;

        // Start the VM
        let start_action = serde_json::json!({
            "action_type": "InstanceStart"
        });

        client
            .put(format!("{}/actions", base_url))
            .json(&start_action)
            .send()
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("VM start failed: {}", e))
            })?;

        Ok(())
    }

    /// Execute Python code in the VM
    pub async fn execute_code(&mut self, code: &str) -> Result<ExecuteResponse, ExecutionError> {
        let process = self
            .process
            .as_mut()
            .ok_or_else(|| ExecutionError::ResourceError("VM process not started".to_string()))?;

        let mut stdin = process
            .stdin
            .take()
            .ok_or_else(|| ExecutionError::ResourceError("Failed to access stdin".to_string()))?;

        let mut stdout = process
            .stdout
            .take()
            .ok_or_else(|| ExecutionError::ResourceError("Failed to access stdout".to_string()))?;

        let mut stderr = process
            .stderr
            .take()
            .ok_or_else(|| ExecutionError::ResourceError("Failed to access stderr".to_string()))?;

        // Prepare Python command
        let python_command = format!("python3 -c '{}'\n", code.replace("'", "\\'"));

        // Send code to VM
        stdin
            .write_all(python_command.as_bytes())
            .await
            .map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to write to stdin: {}", e))
            })?;

        // Close stdin to signal end of input
        drop(stdin);

        // Read output with timeout
        let execution_timeout = Duration::from_secs(30);

        let read_output = timeout(execution_timeout, async {
            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();

            // Read stdout and stderr concurrently
            let stdout_task = async { stdout.read_to_end(&mut stdout_buf).await };

            let stderr_task = async { stderr.read_to_end(&mut stderr_buf).await };

            let (stdout_result, stderr_result) = tokio::join!(stdout_task, stderr_task);

            stdout_result.map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to read stdout: {}", e))
            })?;
            stderr_result.map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to read stderr: {}", e))
            })?;

            Ok((stdout_buf, stderr_buf))
        });

        match read_output.await {
            Ok(Ok((stdout_buf, stderr_buf))) => {
                let stdout_str = String::from_utf8_lossy(&stdout_buf).to_string();
                let stderr_str = String::from_utf8_lossy(&stderr_buf).to_string();

                Ok(create_success_response(stdout_str, stderr_str))
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(ExecutionError::TimeoutError),
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
        assert!(vm_manager.socket_path.contains(&vm_manager.vm_id));
        assert!(vm_manager.socket_path.starts_with("/tmp/firecracker-"));
        assert!(vm_manager.socket_path.ends_with(".socket"));
    }

    #[tokio::test]
    async fn test_vm_manager_cleanup() {
        let vm_manager = VMManager::new();
        let socket_path = vm_manager.socket_path.clone();

        // Create a dummy socket file for testing
        std::fs::write(&socket_path, "test").unwrap();
        assert!(std::path::Path::new(&socket_path).exists());

        // Test cleanup
        let result = vm_manager.cleanup().await;
        assert!(result.is_ok());
        assert!(!std::path::Path::new(&socket_path).exists());
    }

    #[test]
    fn test_python_command_escaping() {
        let code = "print('hello world')";
        let escaped = format!("python3 -c '{}'\n", code.replace("'", "\\'"));
        assert_eq!(escaped, "python3 -c 'print(\\'hello world\\')'\n");
    }

    #[test]
    fn test_complex_python_code_escaping() {
        let code = "print('It\\'s a test'); print(\"Double quotes\")";
        let escaped = format!("python3 -c '{}'\n", code.replace("'", "\\'"));
        assert_eq!(
            escaped,
            "python3 -c 'print(\\'It\\\\'s a test\\'); print(\"Double quotes\")'\n"
        );
    }
}
