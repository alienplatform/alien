use std::{collections::BTreeSet, time::Duration};

use alien_test::TestDeployment;
use anyhow::{bail, Context};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};
use url::Url;

const SHORT_CONNECTIONS: u64 = 80;

pub async fn check_tcp_transaction(deployment: &TestDeployment) -> anyhow::Result<()> {
    let endpoint = deployment
        .url
        .as_deref()
        .context("running TCP deployment did not publish an endpoint")?;
    let address = endpoint_address(endpoint)?;

    let mut persistent = connect(&address).await?;
    let persistent_identity = transact(&mut persistent, 0, "persistent-start").await?;

    let mut identities = BTreeSet::new();
    for sequence in 1..=SHORT_CONNECTIONS {
        let mut connection = connect(&address).await?;
        identities.insert(transact(&mut connection, sequence, "short").await?);
    }
    if identities.len() != 2 {
        bail!(
            "expected exactly two TCP replicas across {SHORT_CONNECTIONS} fresh connections, observed {identities:?}"
        );
    }

    let final_identity = transact(&mut persistent, SHORT_CONNECTIONS + 1, "persistent-end").await?;
    if final_identity != persistent_identity {
        bail!(
            "persistent TCP connection changed replica from {persistent_identity} to {final_identity}"
        );
    }

    assert_private_port_is_closed(endpoint).await?;
    Ok(())
}

async fn connect(address: &str) -> anyhow::Result<BufReader<TcpStream>> {
    let stream = tokio::time::timeout(Duration::from_secs(20), TcpStream::connect(address))
        .await
        .context("timed out connecting to public TCP endpoint")?
        .with_context(|| format!("failed to connect to public TCP endpoint {address}"))?;
    Ok(BufReader::new(stream))
}

async fn transact(
    connection: &mut BufReader<TcpStream>,
    sequence: u64,
    payload: &str,
) -> anyhow::Result<String> {
    let request = format!("TX {sequence} {payload}\n");
    connection
        .get_mut()
        .write_all(request.as_bytes())
        .await
        .context("failed to write TCP transaction")?;

    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(20), connection.read_line(&mut response))
        .await
        .context("timed out waiting for TCP transaction response")??;
    let fields: Vec<&str> = response.trim_end().splitn(5, ' ').collect();
    if fields.len() != 5
        || fields[0] != "ACK"
        || fields[2] != "v1"
        || fields[3] != sequence.to_string()
        || fields[4] != payload
    {
        bail!("unexpected TCP transaction response: {response:?}");
    }
    Ok(fields[1].to_string())
}

fn endpoint_address(endpoint: &str) -> anyhow::Result<String> {
    let url = Url::parse(endpoint).context("published TCP endpoint is not a valid URI")?;
    if url.scheme() != "tcp" {
        bail!("published endpoint uses {}, expected tcp", url.scheme());
    }
    let host = url
        .host_str()
        .context("published TCP endpoint has no host")?;
    let port = url.port().context("published TCP endpoint has no port")?;
    if host.contains(':') {
        Ok(format!("[{}]:{port}", host.trim_matches(['[', ']'])))
    } else {
        Ok(format!("{host}:{port}"))
    }
}

async fn assert_private_port_is_closed(endpoint: &str) -> anyhow::Result<()> {
    let url = Url::parse(endpoint)?;
    let host = url
        .host_str()
        .context("published TCP endpoint has no host")?;
    match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect((host, 8080))).await {
        Ok(Ok(_)) => bail!("private health port 8080 is externally reachable"),
        Ok(Err(_)) | Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_tcp_endpoint_address() {
        assert_eq!(
            endpoint_address("tcp://example.test:7000").expect("valid endpoint"),
            "example.test:7000"
        );
        assert!(endpoint_address("https://example.test:7000").is_err());
        assert_eq!(
            endpoint_address("tcp://[2001:db8::1]:7000").expect("valid IPv6 endpoint"),
            "[2001:db8::1]:7000"
        );
    }
}
