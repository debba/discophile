//! One desktop process per application data directory. The OS releases the lock even after a crash;
//! later launches ask the owner to show its window before exiting, without loading application state.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::tasks::{Tasks, update};

const TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Serialize, Deserialize)]
struct Endpoint {
    port: u16,
    token: [u8; 16],
}

#[derive(Default)]
struct Activation {
    tasks: Option<Tasks>,
    pending: bool,
}

impl Activation {
    fn request(&mut self) {
        if let Some(tasks) = &self.tasks {
            tasks.send(update(|app| {
                super::tray::show_window(app.ctx());
                if let Some(tray) = &mut app.tray {
                    tray.hidden = false;
                }
            }));
        } else {
            // A second launch can arrive before the first window has finished initializing.
            self.pending = true;
        }
    }
}

pub struct SingleInstance {
    _lock: File,
    endpoint_path: PathBuf,
    activation: Arc<Mutex<Activation>>,
    listener: tokio::task::JoinHandle<()>,
}

impl SingleInstance {
    /// Returns `None` after notifying an existing process. Errors stop startup rather than allowing
    /// another process to write the same settings and playback state.
    pub fn acquire() -> io::Result<Option<Self>> {
        Self::acquire_in(&super::storage::data_dir())
    }

    fn acquire_in(directory: &Path) -> io::Result<Option<Self>> {
        std::fs::create_dir_all(directory)?;
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(directory.join("instance.lock"))?;
        let endpoint_path = directory.join("instance.json");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match lock.try_lock() {
                Ok(()) => return Self::start(lock, endpoint_path).map(Some),
                Err(TryLockError::Error(error)) => return Err(error),
                Err(TryLockError::WouldBlock) => {
                    // The owner may still be publishing its endpoint, or exiting. Retry the lock
                    // too, so stale endpoint files never prevent startup after a crash.
                    match notify(&endpoint_path) {
                        Ok(()) => return Ok(None),
                        Err(error) if Instant::now() >= deadline => return Err(error),
                        Err(_) => std::thread::sleep(Duration::from_millis(50)),
                    }
                }
            }
        }
    }

    fn start(lock: File, endpoint_path: PathBuf) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let endpoint = Endpoint { port: listener.local_addr()?.port(), token: *uuid::Uuid::new_v4().as_bytes() };
        // Keep metadata separate: Windows file locks also prohibit reading the locked file.
        std::fs::write(&endpoint_path, serde_json::to_vec(&endpoint)?)?;
        let activation = Arc::new(Mutex::new(Activation::default()));
        let runtime = super::runtime();
        let _entered = runtime.enter();
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let requests = activation.clone();
        let listener = runtime.spawn(async move {
            loop {
                let (mut stream, _) = match listener.accept().await {
                    Ok(connection) => connection,
                    Err(error) => {
                        log::warn!("Unable to accept window activation: {error}");
                        break;
                    }
                };
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut token = [0; 16];
                if !matches!(tokio::time::timeout(TIMEOUT, stream.read_exact(&mut token)).await, Ok(Ok(_))) || token != endpoint.token {
                    continue;
                }
                requests.lock().unwrap().request();
                let _ = tokio::time::timeout(TIMEOUT, stream.write_all(&[1])).await;
            }
        });
        Ok(Self { _lock: lock, endpoint_path, activation, listener })
    }

    /// Connects the listener to the UI and delivers any launch received during startup.
    pub fn attach(&self, tasks: Tasks) {
        let mut activation = self.activation.lock().unwrap();
        activation.tasks = Some(tasks);
        if std::mem::take(&mut activation.pending) {
            activation.request();
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        self.listener.abort();
        let _ = std::fs::remove_file(&self.endpoint_path);
        // Never delete the lock file: another process could otherwise lock a different inode.
    }
}

fn notify(path: &Path) -> io::Result<()> {
    let endpoint: Endpoint = serde_json::from_slice(&std::fs::read(path)?)?;
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
    let mut stream = TcpStream::connect_timeout(&address, TIMEOUT)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.write_all(&endpoint.token)?;
    let mut ack = [0];
    stream.read_exact(&mut ack)?;
    if ack != [1] {
        return Err(io::Error::other("Invalid window activation reply"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn subprocess(directory: &Path, action: &str) {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "platform::single_instance::tests::child_process", "--nocapture"])
            .env("DISCOPHILE_INSTANCE_TEST_DIR", directory)
            .env("DISCOPHILE_INSTANCE_TEST_ACTION", action)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn child_process() {
        let Some(directory) = std::env::var_os("DISCOPHILE_INSTANCE_TEST_DIR") else { return };
        let instance = SingleInstance::acquire_in(Path::new(&directory)).unwrap();
        if std::env::var("DISCOPHILE_INSTANCE_TEST_ACTION").as_deref() == Ok("notify") {
            assert!(instance.is_none());
        } else {
            assert!(instance.is_some());
            // Simulate a crash: skip SingleInstance::drop and leave the endpoint file behind.
            std::process::exit(0);
        }
    }

    #[test]
    fn separate_process_notifies_owner_and_can_restart_after_a_crash() {
        let directory = tempfile::tempdir().unwrap();
        let instance = SingleInstance::acquire_in(directory.path()).unwrap().unwrap();
        let (tasks, updates) = Tasks::new(egui::Context::default());
        instance.attach(tasks);
        subprocess(directory.path(), "notify");
        assert!(updates.recv_timeout(TIMEOUT).is_ok());
        drop(instance);

        subprocess(directory.path(), "crash");
        assert!(directory.path().join("instance.json").exists());
        assert!(SingleInstance::acquire_in(directory.path()).unwrap().is_some());
    }

    #[test]
    fn later_launch_restores_the_existing_window() {
        let directory = tempfile::tempdir().unwrap();
        let instance = SingleInstance::acquire_in(directory.path()).unwrap().unwrap();
        let mut app = crate::App::connect_test_app();
        let ctx = app.ctx().clone();
        let (tasks, updates) = Tasks::new(ctx.clone());
        instance.attach(tasks);

        assert!(SingleInstance::acquire_in(directory.path()).unwrap().is_none());
        let mut output = ctx.run_ui(egui::RawInput::default(), |_ui| {
            updates.recv_timeout(TIMEOUT).unwrap()(&mut app);
        });
        output.textures_delta.clear();
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(commands.contains(&egui::ViewportCommand::Visible(true)));
        assert!(commands.contains(&egui::ViewportCommand::Minimized(false)));
        assert!(commands.contains(&egui::ViewportCommand::Focus));
        assert!(updates.try_recv().is_err());
    }

    #[test]
    fn launch_during_startup_is_delivered_when_ui_is_ready() {
        let directory = tempfile::tempdir().unwrap();
        let instance = SingleInstance::acquire_in(directory.path()).unwrap().unwrap();
        assert!(SingleInstance::acquire_in(directory.path()).unwrap().is_none());
        let (tasks, updates) = Tasks::new(egui::Context::default());
        instance.attach(tasks);
        assert!(updates.try_recv().is_ok());
    }

    #[test]
    fn exit_releases_the_lock_and_stale_metadata_does_not_block_startup() {
        let directory = tempfile::tempdir().unwrap();
        let instance = SingleInstance::acquire_in(directory.path()).unwrap().unwrap();
        drop(instance);
        let path = directory.path().join("instance.json");
        std::fs::write(path, b"stale metadata").unwrap();
        assert!(SingleInstance::acquire_in(directory.path()).unwrap().is_some());
    }

    #[test]
    fn unrelated_local_connections_cannot_activate_the_window() {
        let directory = tempfile::tempdir().unwrap();
        let instance = SingleInstance::acquire_in(directory.path()).unwrap().unwrap();
        let (tasks, updates) = Tasks::new(egui::Context::default());
        instance.attach(tasks);
        let path = directory.path().join("instance.json");
        let endpoint: Endpoint = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)).unwrap();
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        stream.write_all(&[0; 16]).unwrap();
        assert!(stream.read_exact(&mut [0]).is_err());
        assert!(updates.try_recv().is_err());
        notify(&path).unwrap();
        assert!(updates.recv_timeout(TIMEOUT).is_ok());
    }
}
