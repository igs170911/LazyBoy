use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub async fn run_output(argv: &[String]) -> Result<std::process::Output, String> {
    if argv.is_empty() {
        return Err("empty command".into());
    }
    Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .await
        .map_err(|error| error.to_string())
}

pub async fn run_stdout_text(argv: &[String]) -> Result<String, String> {
    let output = run_output(argv).await?;
    Ok(if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).into_owned()
    } else {
        String::from_utf8_lossy(&output.stdout).into_owned()
    })
}

pub async fn run_stdout_text_timeout(argv: &[String], timeout_ms: u64) -> Result<String, String> {
    tokio::time::timeout(
        Duration::from_millis(timeout_ms.max(100)),
        run_stdout_text(argv),
    )
    .await
    .map_err(|_| "timed out".to_string())?
}

pub async fn capture_stdout(argv: &[String]) -> Result<Vec<u8>, String> {
    if argv.is_empty() {
        return Err("empty command".into());
    }
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout)
            .await
            .map_err(|error| error.to_string())?;
    }
    let status = child.wait().await.map_err(|error| error.to_string())?;
    if !status.success() || stdout.is_empty() {
        return Err("command failed".into());
    }
    Ok(stdout)
}

pub async fn spawn_detached(argv: &[String]) -> Result<(), String> {
    if argv.is_empty() {
        return Err("empty command".into());
    }
    Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}
