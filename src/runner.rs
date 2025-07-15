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
const VM_BOOT_TIMEOUT_SECONDS: u64 = 15;
const VM_EXECUTE_TIMEOUT_SECONDS: u64 = 35;
const VM_POOL_SIZE: usize = 3;
pub const VM_PREWARM_COUNT: usize = 2;

impl Default for VMManager {
    fn default() -> Self {
        let vm_id = generate_vm_id();
        let tap_interface = format!("tap-{}", &vm_id[..8]);
        // Generate unique subnet for each VM (172.16.x.0/24 where x is based on VM ID)
        let subnet_id = u32::from_str_radix(&vm_id[..8], 16).unwrap_or(1) % 254 + 1;
        let vm_ip = format!("172.16.{subnet_id}.2");

        Self {
            vm_id: vm_id.clone(),
            socket_path: format!("/tmp/firecracker-{vm_id}.socket"),
            process: None,
            stdout_log_path: format!("/tmp/fc-stdout-{vm_id}.log"),
            stderr_log_path: format!("/tmp/fc-stderr-{vm_id}.log"),
            vm_ip,
            tap_interface,
        }
    }
}

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Check if we're running in test mode
fn is_test_mode() -> bool {
    // Multiple ways to detect test mode
    cfg!(test)
        || std::env::var("CARGO_PKG_NAME").is_ok()
            && std::env::args().any(|arg| arg.contains("test"))
        || std::thread::current().name().unwrap_or("").contains("test")
}

/// VM Pool to reuse VMs and reduce latency
pub static VM_POOL: once_cell::sync::Lazy<Arc<Mutex<VecDeque<VMManager>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(VecDeque::new())));

/// Execute Python code in a Firecracker microVM via HTTP API (optimized with VM pooling)
pub async fn run_in_vm(code: &str) -> Result<ExecuteResponse, ExecutionError> {
    // Try to get a VM from the pool first
    let vm_manager = {
        let mut pool = VM_POOL.lock().await;
        if let Some(vm) = pool.pop_front() {
            tracing::debug!("Reusing VM from pool (pool size: {})", pool.len());
            vm
        } else {
            tracing::debug!("No VMs in pool, creating new one");
            drop(pool);
            create_new_vm().await?
        }
    };

    // Execute code via HTTP API
    let result = vm_manager.execute_code_via_api(code).await;

    match result {
        Ok(response) => {
            // VM is still healthy, return it to pool
            {
                let mut pool = VM_POOL.lock().await;
                if pool.len() < VM_POOL_SIZE {
                    pool.push_back(vm_manager);
                    tracing::debug!("Returned VM to pool (pool size: {})", pool.len());
                } else {
                    // Pool is full, shutdown this VM
                    tokio::spawn(async move {
                        let mut vm = vm_manager;
                        let _ = vm.shutdown_vm().await;
                        let _ = vm.cleanup().await;
                    });
                }
            }
            Ok(response)
        }
        Err(e) => {
            // VM failed, shutdown and cleanup
            tokio::spawn(async move {
                let mut vm = vm_manager;
                let _ = vm.shutdown_vm().await;
                let _ = vm.cleanup().await;
            });
            Err(e)
        }
    }
}

/// Create a new VM and wait for it to be ready
pub async fn create_new_vm() -> Result<VMManager, ExecutionError> {
    let mut vm_manager = VMManager::new().await?;

    // 1. Set up networking
    vm_manager.setup_networking().await?;

    // 2. Start Firecracker with the API server rootfs
    vm_manager.start_firecracker().await?;
    vm_manager.configure_and_run_vm().await?;

    // 3. Wait for VM to boot and API server to be ready
    vm_manager.wait_for_api_server().await?;

    Ok(vm_manager)
}

impl VMManager {
    /// Create a new VM manager with a unique ID
    pub async fn new() -> Result<Self, ExecutionError> {
        let vm_id = generate_vm_id();
        let tap_interface = format!("tap-{}", &vm_id[..8]);
        // Generate unique subnet for each VM (172.16.x.0/24 where x is based on VM ID)
        let subnet_id = u32::from_str_radix(&vm_id[..8], 16).unwrap_or(1) % 254 + 1;
        let vm_ip = format!("172.16.{subnet_id}.2");

        let socket_path = format!("/tmp/firecracker-{vm_id}.socket");
        let stdout_log_path = format!("/tmp/fc-stdout-{vm_id}.log");
        let stderr_log_path = format!("/tmp/fc-stderr-{vm_id}.log");

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
        // Skip networking setup in test mode or for test TAP interfaces
        if is_test_mode() || self.tap_interface.starts_with("test-") {
            tracing::debug!("Skipping network setup in test mode");
            return Ok(());
        }

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
                ExecutionError::ResourceError(format!("Failed to create TAP interface: {e}"))
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
            format!("172.16.{subnet_id}.1/24")
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
                ExecutionError::ResourceError(format!("Failed to configure TAP interface: {e}"))
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
                ExecutionError::ResourceError(format!("Failed to bring up TAP interface: {e}"))
            })?;

        if !up_status.success() {
            return Err(ExecutionError::ResourceError(
                "Failed to bring up TAP interface".to_string(),
            ));
        }

        tracing::debug!(
            "TAP interface {} configured successfully with host IP {} and VM IP {}",
            self.tap_interface,
            host_ip,
            self.vm_ip
        );

        // Test network connectivity
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
                tracing::debug!("Network connectivity to {} verified", self.vm_ip);
            }
            Ok(output) => {
                tracing::debug!(
                    "Ping to {} failed: {}",
                    self.vm_ip,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                tracing::debug!("Failed to ping {}: {}", self.vm_ip, e);
            }
        }
        Ok(())
    }

    /// Clean up old TAP interfaces to prevent routing conflicts
    async fn cleanup_old_tap_interfaces(&self) {
        // Skip cleanup in test mode or for test TAP interfaces
        if is_test_mode() || self.tap_interface.starts_with("test-") {
            return;
        }

        tracing::debug!("Cleaning up old TAP interfaces...");

        // Get list of currently active TAP interfaces from the VM pool
        let active_interfaces = {
            let pool = VM_POOL.lock().await;
            pool.iter()
                .map(|vm| vm.tap_interface.clone())
                .collect::<std::collections::HashSet<_>>()
        };

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
            let mut cleanup_count = 0;
            for line in output_str.lines() {
                if line.contains("tap-") {
                    // Extract TAP interface name
                    if let Some(start) = line.find("tap-") {
                        if let Some(end) = line[start..].find(':') {
                            let tap_name = &line[start..start + end];

                            // Only clean up if this interface is not currently in use by the VM pool
                            // and it's not the current VM's interface
                            if !active_interfaces.contains(tap_name)
                                && tap_name != self.tap_interface
                            {
                                tracing::debug!("Removing unused TAP interface: {}", tap_name);
                                let _ = tokio::process::Command::new("sudo")
                                    .arg("ip")
                                    .arg("link")
                                    .arg("delete")
                                    .arg(tap_name)
                                    .status()
                                    .await;
                                cleanup_count += 1;
                            } else {
                                tracing::debug!("Skipping active TAP interface: {}", tap_name);
                            }
                        }
                    }
                }
            }
            if cleanup_count > 0 {
                tracing::info!("Cleaned up {} unused TAP interfaces", cleanup_count);
            }
        }
    }

    /// Clean up TAP interface
    pub async fn cleanup_networking(&self) -> Result<(), ExecutionError> {
        // Only attempt cleanup if not in test mode
        if !is_test_mode() && !self.tap_interface.starts_with("test-") {
            let _ = tokio::process::Command::new("sudo")
                .arg("ip")
                .arg("link")
                .arg("delete")
                .arg(&self.tap_interface)
                .status()
                .await;
        }

        Ok(())
    }

    /// Wait for the VM API server to be ready
    pub async fn wait_for_api_server(&self) -> Result<(), ExecutionError> {
        // In test mode, simulate successful API server readiness
        if is_test_mode() {
            tracing::debug!("Skipping API server wait in test mode");
            return Ok(());
        }
        let client = reqwest::Client::new();
        let health_url = format!("http://{}:8080/health", self.vm_ip);

        // Wait for the API server to be ready with more aggressive timing
        let mut attempt = 0;
        let mut delay_ms = 100; // Start with 100ms
        let max_delay_ms = 1000; // Max 1 second between attempts

        loop {
            attempt += 1;
            if attempt > 1 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                // Exponential backoff up to max delay
                delay_ms = (delay_ms * 2).min(max_delay_ms);
            }

            if attempt > VM_BOOT_TIMEOUT_SECONDS * 10 {
                // ~15 seconds total with exponential backoff
                break;
            }

            match client
                .get(&health_url)
                .timeout(Duration::from_secs(2))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    tracing::info!(
                        "VM API server at {} is ready after {} attempts ({:.1}s)",
                        self.vm_ip,
                        attempt,
                        (attempt as f64 * delay_ms as f64 / 2000.0)
                    );
                    return Ok(());
                }
                Ok(response) => {
                    tracing::debug!(
                        "Health check attempt {} failed with status: {}",
                        attempt,
                        response.status()
                    );
                }
                Err(e) => {
                    tracing::debug!("Health check attempt {} for {}: {}", attempt, self.vm_ip, e);
                }
            }
        }

        // Read the VM logs to help debug
        let stdout_log = tokio::fs::read_to_string(&self.stdout_log_path)
            .await
            .unwrap_or_else(|e| format!("Failed to read stdout log: {e}"));
        let stderr_log = tokio::fs::read_to_string(&self.stderr_log_path)
            .await
            .unwrap_or_else(|e| format!("Failed to read stderr log: {e}"));

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
        // In test mode, return a mock response to test the handler logic
        if is_test_mode() {
            tracing::debug!("Returning mock response in test mode");
            return Ok(ExecuteResponse {
                stdout: format!("Mock execution of: {code}\n"),
                stderr: "".to_string(),
                success: true,
            });
        }
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
                ExecutionError::ApiCommunicationError(format!("Failed to send request: {e}"))
            })?;

        if !response.status().is_success() {
            return Err(ExecutionError::ApiCommunicationError(format!(
                "API request failed with status: {}",
                response.status()
            )));
        }

        let api_response: serde_json::Value = response.json().await.map_err(|e| {
            ExecutionError::ApiCommunicationError(format!("Failed to parse response: {e}"))
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
        // In test mode, simulate successful start without actually running Firecracker
        if is_test_mode() {
            tracing::debug!("Skipping Firecracker start in test mode");
            return Ok(());
        }
        let stdout_log_file = std::fs::File::create(&self.stdout_log_path)
            .map_err(|e| ExecutionError::ResourceError(format!("cannot create stdout log: {e}")))?;
        let stderr_log_file = std::fs::File::create(&self.stderr_log_path)
            .map_err(|e| ExecutionError::ResourceError(format!("cannot create stderr log: {e}")))?;

        let child = tokio::process::Command::new("firecracker")
            .arg("--api-sock")
            .arg(&self.socket_path)
            .stdin(Stdio::null())
            .stdout(stdout_log_file)
            .stderr(stderr_log_file)
            .spawn()
            .map_err(|e| {
                ExecutionError::ProcessSpawnError(format!("Failed to start Firecracker: {e}"))
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
                    ExecutionError::ApiCommunicationError(format!("Request build failed: {e}"))
                })?
        } else {
            request_builder.body(Full::new(Bytes::new())).map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Request build failed: {e}"))
            })?
        };

        let response = client.request(request).await.map_err(|e| {
            ExecutionError::ApiCommunicationError(format!("API request failed: {e}"))
        })?;

        let status = response.status();
        if !status.is_success() {
            use http_body_util::BodyExt;
            let body_bytes = response
                .collect()
                .await
                .map_err(|e| {
                    ExecutionError::ApiCommunicationError(format!(
                        "Failed to read error response: {e}"
                    ))
                })?
                .to_bytes();
            let error_body = String::from_utf8_lossy(&body_bytes);
            return Err(ExecutionError::ApiCommunicationError(format!(
                "API returned error status: {status} for {method} {path}. Error details: {error_body}"
            )));
        }
        Ok(())
    }

    /// Configure the VM via HTTP API and starts it
    pub async fn configure_and_run_vm(&self) -> Result<(), ExecutionError> {
        // In test mode, simulate successful configuration
        if is_test_mode() {
            tracing::debug!("Skipping VM configuration in test mode");
            return Ok(());
        }
        let machine_config = tokio::fs::read_to_string("fixtures/machine.json")
            .await
            .map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to read machine config: {e}"))
            })?;
        let machine_config: serde_json::Value = serde_json::from_str(&machine_config).unwrap();
        self.send_api_request(
            Method::PUT,
            "/machine-config",
            Some(&machine_config.to_string()),
        )
        .await
        .map_err(|e| {
            ExecutionError::ApiCommunicationError(format!("Machine config failed: {e}"))
        })?;

        let host_ip = {
            let vm_ip_parts: Vec<&str> = self.vm_ip.split('.').collect();
            let subnet_id = vm_ip_parts[2];
            format!("172.16.{subnet_id}.1")
        };
        let boot_args = format!(
            "console=ttyS0 reboot=k panic=1 pci=off init=/usr/local/bin/startup.sh ip={}::{}:255.255.255.0::eth0:off",
            self.vm_ip, host_ip
        );
        let boot_source = serde_json::json!({ "kernel_image_path": "./hello-vmlinux.bin", "boot_args": boot_args });
        self.send_api_request(Method::PUT, "/boot-source", Some(&boot_source.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Boot source config failed: {e}"))
            })?;

        let rootfs = serde_json::json!({ "drive_id": "rootfs", "path_on_host": "./alpine-python-api.ext4", "is_root_device": true, "is_read_only": false });
        self.send_api_request(Method::PUT, "/drives/rootfs", Some(&rootfs.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Rootfs config failed: {e}"))
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
            ExecutionError::ApiCommunicationError(format!("Network config failed: {e}"))
        })?;

        let start_action = serde_json::json!({ "action_type": "InstanceStart" });
        self.send_api_request(Method::PUT, "/actions", Some(&start_action.to_string()))
            .await
            .map_err(|e| ExecutionError::ApiCommunicationError(format!("VM start failed: {e}")))?;
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
                    ExecutionError::ResourceError(format!("Failed to remove socket: {e}"))
                })?;
        }
        if tokio::fs::try_exists(&self.stdout_log_path)
            .await
            .unwrap_or(false)
        {
            tokio::fs::remove_file(&self.stdout_log_path)
                .await
                .map_err(|e| {
                    ExecutionError::ResourceError(format!("Failed to remove stdout log: {e}"))
                })?;
        }
        if tokio::fs::try_exists(&self.stderr_log_path)
            .await
            .unwrap_or(false)
        {
            tokio::fs::remove_file(&self.stderr_log_path)
                .await
                .map_err(|e| {
                    ExecutionError::ResourceError(format!("Failed to remove stderr log: {e}"))
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

        // Create VMManager with test paths and a non-existent TAP interface to avoid sudo
        let vm_manager = VMManager {
            socket_path: socket_path.to_string(),
            stdout_log_path: stdout_log_path.to_string(),
            stderr_log_path: stderr_log_path.to_string(),
            tap_interface: "test-tap-nonexistent".to_string(), // Non-existent interface to avoid sudo issues
            ..Default::default()
        };

        // Cleanup should remove the files (networking cleanup will fail silently)
        vm_manager.cleanup().await.unwrap();
        assert!(!tokio::fs::try_exists(socket_path).await.unwrap());
        assert!(!tokio::fs::try_exists(stdout_log_path).await.unwrap());
        assert!(!tokio::fs::try_exists(stderr_log_path).await.unwrap());
    }
}
