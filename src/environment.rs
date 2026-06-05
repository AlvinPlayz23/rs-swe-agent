use crate::types::CommandOutput;
use anyhow::Result;
use std::{env, path::PathBuf, process::Stdio, time::Duration};
use tokio::process::Command;

#[derive(Clone)]
pub struct LocalEnvironment {
    cwd: PathBuf,
    timeout: Duration,
}

impl LocalEnvironment {
    pub fn new() -> Result<Self> {
        Ok(Self {
            cwd: env::current_dir()?,
            timeout: Duration::from_secs(30),
        })
    }

    pub async fn execute(&self, command: &str) -> CommandOutput {
        let child = match Command::new("bash")
            .arg("-lc")
            .arg(command)
            .current_dir(&self.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                return CommandOutput {
                    output: String::new(),
                    returncode: -1,
                    exception_info: e.to_string(),
                }
            }
        };

        match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Ok(Ok(out)) => {
                let mut text = String::from_utf8_lossy(&out.stdout).to_string();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                CommandOutput {
                    output: text,
                    returncode: out.status.code().unwrap_or(-1),
                    exception_info: String::new(),
                }
            }
            Ok(Err(e)) => CommandOutput {
                output: String::new(),
                returncode: -1,
                exception_info: e.to_string(),
            },
            Err(_) => CommandOutput {
                output: String::new(),
                returncode: -1,
                exception_info: "command timed out".into(),
            },
        }
    }
}
