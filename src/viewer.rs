use anyhow::Result;
use serde_json::{Value, json};
#[cfg(not(test))]
use anyhow::{Context, bail};
#[cfg(all(unix, not(test)))]
use std::os::unix::process::CommandExt;
#[cfg(not(test))]
use std::{
    env,
    process::{Command as ProcessCommand, Stdio},
    time::Duration,
};
use std::{
    net::{TcpListener, TcpStream},
    path::Path,
};

use crate::{ResearchConfig, State};

pub(crate) fn ensure_viewer<S, E>(
    root: &Path,
    config: &ResearchConfig,
    state: &mut State,
    mut save_state: S,
    mut event: E,
) -> Result<()>
where
    S: FnMut(&State) -> Result<()>,
    E: FnMut(&str, &str, Value) -> Result<()>,
{
    #[cfg(test)]
    {
        let _ = (root, config, state, &mut save_state, &mut event);
        return Ok(());
    }
    #[cfg(not(test))]
    {
        if let Some(url) = state.viewer_url.as_deref() {
            if crate::viewer_responds(url, config.timeouts.viewer_timeout_ms) {
                return Ok(());
            }
        }
        let host = config
            .serve_host
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port = config
            .serve_port
            .unwrap_or_else(|| crate::reserve_viewer_port(&host).unwrap_or(0));
        if port == 0 {
            bail!("failed to reserve a local graph viewer port");
        }
        let url = format!("http://{host}:{port}");
        let mut command = ProcessCommand::new(
            env::current_exe().context("failed to locate research binary")?,
        );
        command
            .arg("--root")
            .arg(root)
            .arg("serve")
            .arg("--topic-id")
            .arg(&state.topic_id)
            .arg("--host")
            .arg(&host)
            .arg("--port")
            .arg(port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            command.process_group(0);
        }
        command
            .spawn()
            .with_context(|| format!("failed to start local graph viewer at {url}"))?;
        for _ in 0..20 {
            if crate::viewer_responds(&url, config.timeouts.viewer_timeout_ms) {
                state.viewer_url = Some(url.clone());
                save_state(state)?;
                event(&state.topic_id, "viewer.started", json!({ "url": url }))?;
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bail!(
            "local graph viewer did not become ready at {url}. \
             If a previous viewer process is still running, kill it with: \
             pkill -f \"research.*serve\""
        );
    }
}

pub(crate) fn serve<F>(
    topic_id: &str,
    host: Option<String>,
    port: Option<u16>,
    config: &ResearchConfig,
    mut handle_http: F,
) -> Result<Value>
where
    F: FnMut(TcpStream, &str) -> Result<()>,
{
    let host = host
        .or_else(|| config.serve_host.clone())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = port.or(config.serve_port).unwrap_or(0);
    let listener = TcpListener::bind(format!("{host}:{port}"))?;
    let url = format!("http://{}", listener.local_addr()?);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "action": "serve",
            "topic_id": topic_id,
            "url": url,
        }))?
    );
    let mut logged_first_error = false;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_http(stream, topic_id) {
                    if !logged_first_error {
                        eprintln!("research viewer request failed: {error}");
                        logged_first_error = true;
                    }
                }
            }
            Err(error) => {
                if !logged_first_error {
                    eprintln!("research viewer connection failed: {error}");
                    logged_first_error = true;
                }
            }
        }
    }
    Ok(json!({ "action": "serve", "topic_id": topic_id, "url": url }))
}
