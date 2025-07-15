use crate::{ExecuteResponse, ExecutionError, generate_vm_id};
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Method, Request, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyperlocal::UnixConnector;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Child;
use tokio::time::timeout;

/// VM Manager for handling Firecracker VM lifecycle with HTTP API
#[allow(dead_code)]
pub struct VMManager {
    vm_id: String,
    socket_path: String,
    process: Option<Child>,
    stdout_log_path: String,
    stderr_log_path: String,
    vm_ip: String,
    tap_interface: String,
}

// Constants
const VM_BOOT_TIMEOUT_SECONDS: u64 = 10;
const VM_EXECUTE_TIMEOUT_SECONDS: u64 = 35;

impl Default for VMManager {
    fn default() -> Self {
        let vm_id = generate_vm_id();
        let tap_interface = format!("tap-{}", &vm_id[..8]);
        // Generate unique subnet for each VM (172.16.x.0/24 where x is based on VM ID)
        let subnet_id = u32::from_str_radix(&vm_id[..8], 16).unwrap_or(1) % 254 + 1;
        let vm_ip = format!("172.16.{}.2", subnet_id);

        Self {
            vm_id: vm_id.clone(),
            socket_path: format!("/tmp/firecracker-{}.socket", vm_id),
            process: None,
            stdout_log_path: format!("/tmp/fc-stdout-{}.log", vm_id),
            stderr_log_path: format!("/tmp/fc-stderr-{}.log", vm_id),
            vm_ip,
            tap_interface,
        }
    }
}

/// Execute Python code in a fresh Firecracker microVM via HTTP API
pub async fn run_in_vm(code: &str) -> Result<ExecuteResponse, ExecutionError> {
    let mut vm_manager = VMManager::new().await?;

    // 1. Set up networking
    vm_manager.setup_networking().await?;

    // 2. Start Firecracker with the API server rootfs
    vm_manager.start_firecracker().await?;
    vm_manager.configure_and_run_vm().await?;

    // 3. Wait for VM to boot and API server to be ready
    vm_manager.wait_for_api_server().await?;

    // 4. Send code via HTTP API
    let result = vm_manager.execute_code_via_api(code).await?;

    // 5. Shutdown VM
    vm_manager.shutdown_vm().await?;

    // 6. Cleanup
    vm_manager.cleanup().await?;

    Ok(result)
}

impl VMManager {
    /// Create a new VM manager with a unique ID
    pub async fn new() -> Result<Self, ExecutionError> {
        let vm_id = generate_vm_id();
        let tap_interface = format!("tap-{}", &vm_id[..8]);
        // Generate unique subnet for each VM (172.16.x.0/24 where x is based on VM ID)
        let subnet_id = u32::from_str_radix(&vm_id[..8], 16).unwrap_or(1) % 254 + 1;
        let vm_ip = format!("172.16.{}.2", subnet_id);

        let socket_path = format!("/tmp/firecracker-{}.socket", vm_id);
        let stdout_log_path = format!("/tmp/fc-stdout-{}.log", vm_id);
        let stderr_log_path = format!("/tmp/fc-stderr-{}.log", vm_id);

        Ok(Self {
            vm_id,
            socket_path,
            process: None,
            stdout_log_path,
            stderr_log_path,
            vm_ip,
            tap_interface,
        })
    }

    /// Set up TAP interface for VM networking with unique subnet
    pub async fn setup_networking(&self) -> Result<(), ExecutionError> {
        // First, clean up any old TAP interfaces that might conflict
        self.cleanup_old_tap_interfaces().await;

        // Create TAP interface
        let tap_status = tokio::process::Command::new("sudo")
            .arg("ip")
            .arg("tuntap")
            .arg("add")
            .arg("dev")
            .arg(&self.tap_interface)
            .arg("mode")
            .arg("tap")
            .status()
            .await
            .map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to create TAP interface: {}", e))
            })?;

        if !tap_status.success() {
            return Err(ExecutionError::ResourceError(
                "Failed to create TAP interface".to_string(),
            ));
        }

        // Configure TAP interface with host IP (VM subnet .1)
        let host_ip = {
            let vm_ip_parts: Vec<&str> = self.vm_ip.split('.').collect();
            let subnet_id = vm_ip_parts[2];
            format!("172.16.{}.1/24", subnet_id)
        };

        let ip_status = tokio::process::Command::new("sudo")
            .arg("ip")
            .arg("addr")
            .arg("add")
            .arg(&host_ip)
            .arg("dev")
            .arg(&self.tap_interface)
            .status()
            .await
            .map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to configure TAP interface: {}", e))
            })?;

        if !ip_status.success() {
            return Err(ExecutionError::ResourceError(
                "Failed to configure TAP interface".to_string(),
            ));
        }

        // Bring TAP interface up
        let up_status = tokio::process::Command::new("sudo")
            .arg("ip")
            .arg("link")
            .arg("set")
            .arg("dev")
            .arg(&self.tap_interface)
            .arg("up")
            .status()
            .await
            .map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to bring up TAP interface: {}", e))
            })?;

        if !up_status.success() {
            return Err(ExecutionError::ResourceError(
                "Failed to bring up TAP interface".to_string(),
            ));
        }

        tracing::info!(
            "TAP interface {} configured successfully with host IP {} and VM IP {}",
            self.tap_interface,
            host_ip,
            self.vm_ip
        );

        // Test network connectivity
        tracing::info!("Testing network connectivity to VM at {}", self.vm_ip);
        let ping_result = tokio::process::Command::new("ping")
            .arg("-c")
            .arg("1")
            .arg("-W")
            .arg("2")
            .arg(&self.vm_ip)
            .output()
            .await;

        match ping_result {
            Ok(output) if output.status.success() => {
                tracing::info!("Ping to {} successful", self.vm_ip);
            }
            Ok(output) => {
                tracing::warn!(
                    "Ping to {} failed: {}",
                    self.vm_ip,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                tracing::warn!("Failed to ping {}: {}", self.vm_ip, e);
            }
        }
        Ok(())
    }

    /// Clean up old TAP interfaces to prevent routing conflicts
    async fn cleanup_old_tap_interfaces(&self) {
        tracing::info!("Cleaning up old TAP interfaces...");

        // Get list of existing TAP interfaces
        let output = tokio::process::Command::new("ip")
            .arg("link")
            .arg("show")
            .arg("type")
            .arg("tun")
            .output()
            .await;

        if let Ok(output) = output {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                if line.contains("tap-") {
                    // Extract TAP interface name
                    if let Some(start) = line.find("tap-") {
                        if let Some(end) = line[start..].find(':') {
                            let tap_name = &line[start..start + end];
                            tracing::info!("Removing old TAP interface: {}", tap_name);
                            let _ = tokio::process::Command::new("sudo")
                                .arg("ip")
                                .arg("link")
                                .arg("delete")
                                .arg(tap_name)
                                .status()
                                .await;
                        }
                    }
                }
            }
        }
    }

    /// Clean up TAP interface
    pub async fn cleanup_networking(&self) -> Result<(), ExecutionError> {
        let _ = tokio::process::Command::new("sudo")
            .arg("ip")
            .arg("link")
            .arg("delete")
            .arg(&self.tap_interface)
            .status()
            .await;

        Ok(())
    }

    /// Wait for the VM API server to be ready
    pub async fn wait_for_api_server(&self) -> Result<(), ExecutionError> {
        let client = reqwest::Client::new();
        let health_url = format!("http://{}:8080/health", self.vm_ip);

        // Wait up to 5 seconds for the API server to be ready
        for attempt in 1..=VM_BOOT_TIMEOUT_SECONDS {
            tokio::time::sleep(Duration::from_secs(1)).await;

            match client
                .get(&health_url)
                .timeout(Duration::from_secs(2))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    tracing::info!(
                        "VM API server at {} is ready after {} seconds",
                        self.vm_ip,
                        attempt
                    );
                    return Ok(());
                }
                Ok(response) => {
                    tracing::info!(
                        "Health check attempt {} failed with status: {}",
                        attempt,
                        response.status()
                    );
                }
                Err(e) => {
                    tracing::info!("Health check attempt {} for {}: {}", attempt, self.vm_ip, e);
                }
            }
        }

        // Read the VM logs to help debug
        let stdout_log = tokio::fs::read_to_string(&self.stdout_log_path)
            .await
            .unwrap_or_else(|e| format!("Failed to read stdout log: {}", e));
        let stderr_log = tokio::fs::read_to_string(&self.stderr_log_path)
            .await
            .unwrap_or_else(|e| format!("Failed to read stderr log: {}", e));

        let log_details = format!(
            "VM API server at {} did not become ready within {} seconds\n\nFirecracker stdout:\n{}\n\nFirecracker stderr:\n{}",
            self.vm_ip, VM_BOOT_TIMEOUT_SECONDS, stdout_log, stderr_log
        );

        Err(ExecutionError::TimeoutErrorWithLogs(log_details))
    }

    /// Execute code via the VM's HTTP API
    pub async fn execute_code_via_api(
        &self,
        code: &str,
    ) -> Result<ExecuteResponse, ExecutionError> {
        let client = reqwest::Client::new();
        let execute_url = format!("http://{}:8080/execute", self.vm_ip);

        let request_body = serde_json::json!({
            "code": code
        });

        let response = client
            .post(&execute_url)
            .json(&request_body)
            .timeout(Duration::from_secs(VM_EXECUTE_TIMEOUT_SECONDS)) // 5 seconds buffer over the VM's 30s timeout
            .send()
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Failed to send request: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(ExecutionError::ApiCommunicationError(format!(
                "API request failed with status: {}",
                response.status()
            )));
        }

        let api_response: serde_json::Value = response.json().await.map_err(|e| {
            ExecutionError::ApiCommunicationError(format!("Failed to parse response: {}", e))
        })?;

        Ok(ExecuteResponse {
            stdout: api_response["stdout"].as_str().unwrap_or("").to_string(),
            stderr: api_response["stderr"].as_str().unwrap_or("").to_string(),
            success: api_response["success"].as_bool().unwrap_or(false),
        })
    }

    /// Shutdown the VM via API
    pub async fn shutdown_vm(&mut self) -> Result<(), ExecutionError> {
        let client = reqwest::Client::new();
        let shutdown_url = format!("http://{}:8080/shutdown", self.vm_ip);

        // Send shutdown request, but don't wait for response since VM will shutdown
        let _ = client
            .post(&shutdown_url)
            .timeout(Duration::from_secs(5))
            .send()
            .await;

        // Wait for the VM process to exit
        if let Some(mut process) = self.process.take() {
            let _ = timeout(Duration::from_secs(10), process.wait()).await;
        }

        Ok(())
    }

    /// Start the Firecracker process
    pub async fn start_firecracker(&mut self) -> Result<(), ExecutionError> {
        let stdout_log_file = std::fs::File::create(&self.stdout_log_path).map_err(|e| {
            ExecutionError::ResourceError(format!("cannot create stdout log: {}", e))
        })?;
        let stderr_log_file = std::fs::File::create(&self.stderr_log_path).map_err(|e| {
            ExecutionError::ResourceError(format!("cannot create stderr log: {}", e))
        })?;

        let child = tokio::process::Command::new("firecracker")
            .arg("--api-sock")
            .arg(&self.socket_path)
            .stdin(Stdio::null())
            .stdout(stdout_log_file)
            .stderr(stderr_log_file)
            .spawn()
            .map_err(|e| {
                ExecutionError::ProcessSpawnError(format!("Failed to start Firecracker: {}", e))
            })?;
        self.process = Some(child);
        tokio::time::sleep(Duration::from_millis(100)).await; // Give time for socket to be created
        Ok(())
    }

    /// Send HTTP request to Firecracker API via Unix socket
    async fn send_api_request(
        &self,
        method: Method,
        path: &str,
        body: Option<&str>,
    ) -> Result<(), ExecutionError> {
        let client: Client<UnixConnector, Full<Bytes>> =
            Client::builder(TokioExecutor::new()).build(UnixConnector);
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

    /// Configure the VM via HTTP API and starts it
    pub async fn configure_and_run_vm(&self) -> Result<(), ExecutionError> {
        let machine_config = tokio::fs::read_to_string("fixtures/machine.json")
            .await
            .map_err(|e| {
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

        let host_ip = {
            let vm_ip_parts: Vec<&str> = self.vm_ip.split('.').collect();
            let subnet_id = vm_ip_parts[2];
            format!("172.16.{}.1", subnet_id)
        };
        let boot_args = format!(
            "console=ttyS0 reboot=k panic=1 pci=off init=/usr/local/bin/startup.sh ip={}::{}:255.255.255.0::eth0:off",
            self.vm_ip, host_ip
        );
        let boot_source = serde_json::json!({ "kernel_image_path": "./hello-vmlinux.bin", "boot_args": boot_args });
        self.send_api_request(Method::PUT, "/boot-source", Some(&boot_source.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Boot source config failed: {}", e))
            })?;

        let rootfs = serde_json::json!({ "drive_id": "rootfs", "path_on_host": "./alpine-python-api.ext4", "is_root_device": true, "is_read_only": false });
        self.send_api_request(Method::PUT, "/drives/rootfs", Some(&rootfs.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Rootfs config failed: {}", e))
            })?;

        // Configure network interface
        let network_config = serde_json::json!({
            "iface_id": "eth0",
            "guest_mac": "AA:FC:00:00:00:01",
            "host_dev_name": self.tap_interface
        });
        self.send_api_request(
            Method::PUT,
            "/network-interfaces/eth0",
            Some(&network_config.to_string()),
        )
        .await
        .map_err(|e| {
            ExecutionError::ApiCommunicationError(format!("Network config failed: {}", e))
        })?;

        let start_action = serde_json::json!({ "action_type": "InstanceStart" });
        self.send_api_request(Method::PUT, "/actions", Some(&start_action.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("VM start failed: {}", e))
            })?;
        Ok(())
    }

    /// Clean up VM resources
    pub async fn cleanup(mut self) -> Result<(), ExecutionError> {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill().await;
            let _ = process.wait().await;
        }

        // Clean up networking
        let _ = self.cleanup_networking().await;

        if tokio::fs::try_exists(&self.socket_path)
            .await
            .unwrap_or(false)
        {
            tokio::fs::remove_file(&self.socket_path)
                .await
                .map_err(|e| {
                    ExecutionError::ResourceError(format!("Failed to remove socket: {}", e))
                })?;
        }
        if tokio::fs::try_exists(&self.stdout_log_path)
            .await
            .unwrap_or(false)
        {
            tokio::fs::remove_file(&self.stdout_log_path)
                .await
                .map_err(|e| {
                    ExecutionError::ResourceError(format!("Failed to remove stdout log: {}", e))
                })?;
        }
        if tokio::fs::try_exists(&self.stderr_log_path)
            .await
            .unwrap_or(false)
        {
            tokio::fs::remove_file(&self.stderr_log_path)
                .await
                .map_err(|e| {
                    ExecutionError::ResourceError(format!("Failed to remove stderr log: {}", e))
                })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vm_manager_creation() {
        let vm_manager = VMManager::default();
        assert!(!vm_manager.vm_id.is_empty());
        assert!(vm_manager.socket_path.contains("/tmp/firecracker-"));
    }

    #[tokio::test]
    async fn test_vm_manager_cleanup() {
        let socket_path = "/tmp/test-socket.socket";
        let stdout_log_path = "/tmp/test-stdout.log";
        let stderr_log_path = "/tmp/test-stderr.log";

        // Create test files
        tokio::fs::File::create(socket_path).await.unwrap();
        tokio::fs::File::create(stdout_log_path).await.unwrap();
        tokio::fs::File::create(stderr_log_path).await.unwrap();

        assert!(tokio::fs::try_exists(socket_path).await.unwrap());
        assert!(tokio::fs::try_exists(stdout_log_path).await.unwrap());
        assert!(tokio::fs::try_exists(stderr_log_path).await.unwrap());

        // Create VMManager with test paths
        let vm_manager = VMManager {
            socket_path: socket_path.to_string(),
            stdout_log_path: stdout_log_path.to_string(),
            stderr_log_path: stderr_log_path.to_string(),
            ..Default::default()
        };

        // Cleanup should remove the files
        vm_manager.cleanup().await.unwrap();
        assert!(!tokio::fs::try_exists(socket_path).await.unwrap());
        assert!(!tokio::fs::try_exists(stdout_log_path).await.unwrap());
        assert!(!tokio::fs::try_exists(stderr_log_path).await.unwrap());
    }
}
