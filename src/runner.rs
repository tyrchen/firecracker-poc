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
use tracing::warn;

/// VM Manager for handling Firecracker VM lifecycle
#[allow(dead_code)]
pub struct VMManager {
    vm_id: String,
    socket_path: String,
    shared_img: String,
    process: Option<Child>,
    stdout_log_path: String,
    stderr_log_path: String,
}

impl Default for VMManager {
    fn default() -> Self {
        // This is now only used for tests that don't need a real image.
        // The main path uses `VMManager::new().await`.
        let vm_id = generate_vm_id();
        Self {
            vm_id: vm_id.clone(),
            socket_path: format!("/tmp/firecracker-{}.socket", vm_id),
            shared_img: format!("/tmp/firecracker-shared-{}.ext4", vm_id),
            process: None,
            stdout_log_path: format!("/tmp/fc-stdout-{}.log", vm_id),
            stderr_log_path: format!("/tmp/fc-stderr-{}.log", vm_id),
        }
    }
}

/// Execute Python code in a fresh Firecracker microVM
pub async fn run_in_vm(code: &str) -> Result<ExecuteResponse, ExecutionError> {
    // The creation of the VMManager, which involves creating a file, is async.
    let mut vm_manager = VMManager::new().await?;

    // 1. Prepare the shared image with the code FIRST
    vm_manager.prepare_shared_image(code).await?;

    // 2. Start Firecracker. The boot args will now auto-execute and shutdown.
    vm_manager.start_firecracker().await?;
    vm_manager.configure_and_run_vm().await?;

    // 3. Wait for the VM process to exit
    if let Some(mut process) = vm_manager.process.take() {
        match timeout(Duration::from_secs(30), process.wait()).await {
            Ok(Ok(exit_status)) => {
                tracing::info!("Firecracker process exited with status: {}", exit_status);
            }
            Ok(Err(e)) => {
                return Err(ExecutionError::ProcessSpawnError(format!(
                    "Failed to wait for Firecracker process: {}",
                    e,
                )));
            }
            Err(_) => {
                // Timeout
                let _ = process.kill().await; // Kill the hanging process
                // Now read the logs
                let stdout_log = tokio::fs::read_to_string(&vm_manager.stdout_log_path)
                    .await
                    .unwrap_or_else(|e| format!("Failed to read stdout log: {}", e));
                let stderr_log = tokio::fs::read_to_string(&vm_manager.stderr_log_path)
                    .await
                    .unwrap_or_else(|e| format!("Failed to read stderr log: {}", e));
                let log_details = format!(
                    "Firecracker stdout (serial console):\n{}\n\nFirecracker stderr:\n{}",
                    stdout_log, stderr_log
                );
                return Err(ExecutionError::TimeoutErrorWithLogs(log_details));
            }
        }
    } else {
        return Err(ExecutionError::ProcessSpawnError(
            "VM process was not started".to_string(),
        ));
    }

    // 4. Get the results from the shared image
    let result = vm_manager.read_results_from_shared_image().await?;

    // 5. Cleanup
    vm_manager.cleanup().await?;

    Ok(result)
}

impl VMManager {
    /// Create a new VM manager with a unique ID and a dedicated shared filesystem image.
    pub async fn new() -> Result<Self, ExecutionError> {
        let vm_id = generate_vm_id();
        let socket_path = format!("/tmp/firecracker-{}.socket", vm_id);
        let shared_img = format!("/tmp/firecracker-shared-{}.ext4", vm_id);
        let stdout_log_path = format!("/tmp/fc-stdout-{}.log", vm_id);
        let stderr_log_path = format!("/tmp/fc-stderr-{}.log", vm_id);

        // Create a small ext4 image (10MB) for file sharing
        let create_img_status = tokio::process::Command::new("dd")
            .args([
                "if=/dev/zero",
                &format!("of={}", shared_img),
                "bs=1M",
                "count=10",
            ])
            .status()
            .await
            .map_err(|e| ExecutionError::ResourceError(format!("Failed to execute dd: {}", e)))?;

        if !create_img_status.success() {
            return Err(ExecutionError::ResourceError(
                "Failed to create shared image file with dd".to_string(),
            ));
        }

        // Format as ext4
        let format_status = tokio::process::Command::new("mkfs.ext4")
            .args(["-F", &shared_img])
            .status()
            .await
            .map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to execute mkfs.ext4: {}", e))
            })?;

        if !format_status.success() {
            return Err(ExecutionError::ResourceError(
                "Failed to format shared image with mkfs.ext4".to_string(),
            ));
        }

        Ok(Self {
            vm_id,
            socket_path,
            shared_img,
            process: None,
            stdout_log_path,
            stderr_log_path,
        })
    }

    /// Changes the ownership of the mounted directory to the current user.
    async fn set_mount_ownership(&self, mount_dir: &str) -> Result<(), ExecutionError> {
        let user_output = tokio::process::Command::new("id")
            .arg("-un")
            .output()
            .await
            .map_err(|e| ExecutionError::ResourceError(format!("Failed to run `id -un`: {}", e)))?;
        if !user_output.status.success() {
            return Err(ExecutionError::ResourceError("`id -un` failed".to_string()));
        }
        let user = String::from_utf8_lossy(&user_output.stdout)
            .trim()
            .to_string();

        let group_output = tokio::process::Command::new("id")
            .arg("-gn")
            .output()
            .await
            .map_err(|e| ExecutionError::ResourceError(format!("Failed to run `id -gn`: {}", e)))?;
        if !group_output.status.success() {
            return Err(ExecutionError::ResourceError("`id -gn` failed".to_string()));
        }
        let group = String::from_utf8_lossy(&group_output.stdout)
            .trim()
            .to_string();

        let chown_status = tokio::process::Command::new("sudo")
            .arg("chown")
            .arg("-R")
            .arg(format!("{}:{}", &user, &group))
            .arg(mount_dir)
            .status()
            .await
            .map_err(|e| ExecutionError::ResourceError(format!("Failed to run chown: {}", e)))?;

        if !chown_status.success() {
            return Err(ExecutionError::ResourceError(
                "Failed to change ownership of mount point".to_string(),
            ));
        }

        Ok(())
    }

    /// Prepares the shared filesystem image by mounting it and writing the Python script.
    pub async fn prepare_shared_image(&self, code: &str) -> Result<(), ExecutionError> {
        let mount_dir = format!("/tmp/mount-{}", self.vm_id);
        tokio::fs::create_dir_all(&mount_dir).await.map_err(|e| {
            ExecutionError::ResourceError(format!("Failed to create mount directory: {}", e))
        })?;

        // Ensure the loop module is loaded. This might fail if it's built-in, which is fine.
        let modprobe_status = tokio::process::Command::new("sudo")
            .arg("modprobe")
            .arg("loop")
            .status()
            .await;
        if let Ok(status) = modprobe_status {
            if !status.success() {
                warn!(
                    "'sudo modprobe loop' failed. This may not be an issue if the module is already loaded or built-in."
                );
            }
        } else {
            warn!("'sudo modprobe loop' failed to execute.");
        }

        let mount_status = tokio::process::Command::new("sudo")
            .arg("mount")
            .args(["-o", "loop", &self.shared_img, &mount_dir])
            .status()
            .await
            .map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to execute mount: {}", e))
            })?;

        if !mount_status.success() {
            let _ = tokio::fs::remove_dir_all(&mount_dir).await;
            return Err(ExecutionError::ResourceError(
                "Failed to mount shared image".to_string(),
            ));
        }

        self.set_mount_ownership(&mount_dir).await?;

        let script_file = format!("{}/script.py", mount_dir);
        tokio::fs::write(&script_file, code).await.map_err(|e| {
            ExecutionError::ResourceError(format!("Failed to write script.py: {}", e))
        })?;

        let umount_status = tokio::process::Command::new("sudo")
            .arg("umount")
            .arg(&mount_dir)
            .status()
            .await
            .map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to execute umount: {}", e))
            })?;

        if !umount_status.success() {
            let _ = tokio::fs::remove_dir_all(&mount_dir).await;
            return Err(ExecutionError::ResourceError(
                "Failed to unmount shared image".to_string(),
            ));
        }

        let _ = tokio::fs::remove_dir_all(&mount_dir).await;
        Ok(())
    }

    /// Reads the execution results from the shared filesystem image after the VM has run.
    pub async fn read_results_from_shared_image(&self) -> Result<ExecuteResponse, ExecutionError> {
        let mount_dir = format!("/tmp/mount-{}", self.vm_id);
        tokio::fs::create_dir_all(&mount_dir).await.map_err(|e| {
            ExecutionError::ResourceError(format!("Failed to recreate mount directory: {}", e))
        })?;

        let mount_status = tokio::process::Command::new("sudo")
            .arg("mount")
            .args(["-o", "loop", &self.shared_img, &mount_dir])
            .status()
            .await
            .map_err(|e| {
                ExecutionError::ResourceError(format!("Failed to execute mount for reading: {}", e))
            })?;

        if !mount_status.success() {
            let _ = tokio::fs::remove_dir_all(&mount_dir).await;
            return Err(ExecutionError::ResourceError(
                "Failed to remount shared image for reading".to_string(),
            ));
        }

        self.set_mount_ownership(&mount_dir).await?;

        let stdout_file = format!("{}/output.txt", mount_dir);
        let stderr_file = format!("{}/error.txt", mount_dir);

        // If neither output nor error file was created, the script likely didn't run.
        if !tokio::fs::try_exists(&stdout_file).await.unwrap_or(false)
            && !tokio::fs::try_exists(&stderr_file).await.unwrap_or(false)
        {
            let _ = tokio::fs::remove_dir_all(&mount_dir).await; // Clean up mount dir
            return Ok(ExecuteResponse {
                stdout: "".to_string(),
                stderr: "Execution failed: output files not found. The VM may have panicked or the script failed to produce output/error files.".to_string(),
                success: false,
            });
        }

        let stdout = tokio::fs::read_to_string(stdout_file)
            .await
            .unwrap_or_default();
        let stderr = tokio::fs::read_to_string(stderr_file)
            .await
            .unwrap_or_default();

        let umount_status = tokio::process::Command::new("sudo")
            .arg("umount")
            .arg(&mount_dir)
            .status()
            .await
            .map_err(|e| {
                ExecutionError::ResourceError(format!(
                    "Failed to execute umount after reading: {}",
                    e
                ))
            })?;

        if !umount_status.success() {
            warn!("Warning: failed to unmount after reading results.");
        }

        let _ = tokio::fs::remove_dir_all(&mount_dir).await;
        let success = stderr.trim().is_empty();
        Ok(ExecuteResponse {
            stdout,
            stderr,
            success,
        })
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
            .stdin(Stdio::piped())
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

        let command = "mkdir -p /mnt/shared; mount /dev/vdb /mnt/shared; python3 /mnt/shared/script.py > /mnt/shared/output.txt 2> /mnt/shared/error.txt; sync; reboot -f";
        // The shell command must be passed as a single token to the kernel.
        // We wrap the command in escaped double-quotes `\"...\"` which are serialized
        // to `\\"...\\"` in JSON. The kernel parses this correctly, passing a single
        // quoted argument to `/bin/sh -c`.
        let boot_args = format!(
            "console=ttyS0 reboot=k panic=1 pci=off init=/bin/sh -c \"{}\"",
            command
        );
        let boot_source = serde_json::json!({ "kernel_image_path": "./hello-vmlinux.bin", "boot_args": boot_args });
        self.send_api_request(Method::PUT, "/boot-source", Some(&boot_source.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Boot source config failed: {}", e))
            })?;

        let rootfs = serde_json::json!({ "drive_id": "rootfs", "path_on_host": "./alpine-python.ext4", "is_root_device": true, "is_read_only": false });
        self.send_api_request(Method::PUT, "/drives/rootfs", Some(&rootfs.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Rootfs config failed: {}", e))
            })?;

        let shared_fs = serde_json::json!({ "drive_id": "shared", "path_on_host": self.shared_img.clone(), "is_root_device": false, "is_read_only": false });
        self.send_api_request(Method::PUT, "/drives/shared", Some(&shared_fs.to_string()))
            .await
            .map_err(|e| {
                ExecutionError::ApiCommunicationError(format!("Shared fs config failed: {}", e))
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
        if tokio::fs::try_exists(&self.shared_img)
            .await
            .unwrap_or(false)
        {
            tokio::fs::remove_file(&self.shared_img)
                .await
                .map_err(|e| {
                    ExecutionError::ResourceError(format!("Failed to remove shared image: {}", e))
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
        // This test requires root to use dd, mkfs.ext4, and mount.
        // It's more of an integration test. For now, we just test the struct creation.
        let vm_manager = VMManager::default();
        assert!(!vm_manager.vm_id.is_empty());
        assert!(vm_manager.socket_path.contains("/tmp/firecracker-"));
        assert!(vm_manager.shared_img.ends_with(".ext4"));
    }

    #[tokio::test]
    async fn test_vm_manager_cleanup() {
        let socket_path = "/tmp/test-socket.socket";
        let shared_img_path = "/tmp/test-shared.ext4";

        // Create test files
        tokio::fs::File::create(socket_path).await.unwrap();
        tokio::fs::File::create(shared_img_path).await.unwrap();
        assert!(tokio::fs::try_exists(socket_path).await.unwrap());
        assert!(tokio::fs::try_exists(shared_img_path).await.unwrap());

        // Create VMManager with test paths
        let vm_manager = VMManager {
            socket_path: socket_path.to_string(),
            shared_img: shared_img_path.to_string(),
            ..Default::default()
        };

        // Cleanup should remove the files
        vm_manager.cleanup().await.unwrap();
        assert!(!tokio::fs::try_exists(socket_path).await.unwrap());
        assert!(!tokio::fs::try_exists(shared_img_path).await.unwrap());
    }
}
