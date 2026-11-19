use anyhow::{Context, Result, bail, ensure};
use clap::{ArgGroup, Parser};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{io::Read, path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(version, about = "Send a message to an agent without waiting for its reply",
    override_usage = "prismagent-sender [--server <URL>] <WORKSPACE_UUID/AGENT_UUID> <MESSAGE|--stdin|--file <FILE>>",
    group(ArgGroup::new("source").required(true).args(["message", "stdin", "file"])))]
struct Args {
    /// Base URL of the PrismAgent daemon
    #[arg(long, default_value = "http://127.0.0.1:7618")]
    server: String,
    #[arg(value_name = "WORKSPACE_UUID/AGENT_UUID", value_parser = parse_address)]
    addr: Address,
    /// UTF-8 message text
    message: Option<String>,
    /// Read the message from standard input
    #[arg(long)]
    stdin: bool,
    /// Read the message from a UTF-8 file
    #[arg(long)]
    file: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct Address {
    workspace: String,
    agent: String,
}

fn parse_address(value: &str) -> Result<Address, String> {
    match value.split_once('/') {
        Some((workspace, agent))
            if !workspace.is_empty() && !agent.is_empty() && !agent.contains('/') =>
        {
            Ok(Address {
                workspace: workspace.into(),
                agent: agent.into(),
            })
        }
        _ => Err("expected <workspace_uuid>/<agent_uuid>".into()),
    }
}

impl Args {
    fn read_message(&self) -> Result<String> {
        let text = if let Some(path) = &self.file {
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?
        } else if self.stdin {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .context("read stdin")?;
            text
        } else {
            self.message.clone().context("missing message")?
        };
        ensure!(!text.trim().is_empty(), "message is empty");
        Ok(text)
    }
}

async fn post(client: &Client, base: &str, path: &str, body: Value) -> Result<Value> {
    let response = client
        .post(format!("{base}/api/{path}"))
        .json(&body)
        .send()
        .await
        .with_context(|| format!("POST {path}"))?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        bail!("{path}: HTTP {status}: {text}");
    }
    serde_json::from_str(&text).with_context(|| format!("invalid JSON from {path}"))
}

#[derive(Deserialize)]
struct Lease {
    lease_token: String,
}

async fn send(args: Args) -> Result<()> {
    // Reading a pipe may take arbitrarily long. Only acquire the lease once input is ready.
    let text = args.read_message()?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?;
    let base = args.server.trim_end_matches('/');
    let workspace = &args.addr.workspace;
    let lease: Lease = serde_json::from_value(post(&client, base, "workspaces/acquire_lease",
        json!({"workspace_uuid": workspace, "client_id": format!("sender-{}", uuid::Uuid::now_v7())})).await?)
        .context("invalid lease response")?;
    let result = post(&client, base, "agents/send_message", json!({
        "workspace_uuid": workspace, "lease_token": lease.lease_token, "agent_uuid": args.addr.agent,
        "message_body": {"text": text, "attachments": []}
    })).await;
    // A cleanup failure must not turn an accepted message into a reason to resend it.
    if let Err(error) = post(
        &client,
        base,
        "workspaces/release_lease",
        json!({"workspace_uuid": workspace, "lease_token": lease.lease_token}),
    )
    .await
    {
        eprintln!("warning: {error:#}");
    }
    ensure!(
        result?["accepted"] == true,
        "daemon did not confirm acceptance"
    );
    println!("accepted");
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    send(Args::parse()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_scoped_address_and_exactly_one_source() {
        for args in [
            vec!["same", "text"],
            vec!["ws/a/b", "text"],
            vec!["ws/", "text"],
            vec!["ws/a"],
            vec!["ws/a", "text", "--stdin"],
            vec!["ws/a", "--stdin", "--file", "f"],
        ] {
            assert!(Args::try_parse_from(std::iter::once("sender").chain(args)).is_err());
        }
        for args in [
            vec!["ws/a", "text"],
            vec!["ws/a", "--stdin"],
            vec!["ws/a", "--file", "f"],
        ] {
            assert!(Args::try_parse_from(std::iter::once("sender").chain(args)).is_ok());
        }
    }
}
