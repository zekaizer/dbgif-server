use anyhow::Result;
use std::process::Stdio;
use tokio::process::{Child, Command};

pub struct ProcessManager {
    processes: Vec<ManagedProcess>,
}

pub struct ManagedProcess {
    name: String,
    child: Child,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: Vec::new(),
        }
    }

    pub async fn spawn_process(&mut self, name: String, command: &str, args: &[&str]) -> Result<()> {
        let child = Command::new(command)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        self.processes.push(ManagedProcess { name, child });
        Ok(())
    }

    pub async fn stop_all(&mut self) -> Result<()> {
        for process in &mut self.processes {
            process.child.kill().await?;
        }
        self.processes.clear();
        Ok(())
    }

    pub fn get_process(&mut self, name: &str) -> Option<&mut ManagedProcess> {
        self.processes.iter_mut().find(|p| p.name == name)
    }
}