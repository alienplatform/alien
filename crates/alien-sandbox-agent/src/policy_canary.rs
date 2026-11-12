//! Proof that the cluster is enforcing this pod's NetworkPolicy before the pod takes a session.
//!
//! On a fresh node the network policy agent can lag behind the first pods scheduled onto it, and
//! a pod that starts in that window has unrestricted egress until its policy is loaded. The
//! Operator therefore names an address the policy must refuse (its own pod address) and the
//! agent tries to connect to it. A connection that is accepted or refused outright means the
//! pod can reach it, so the policy is not in force yet. A connection that is dropped (it times
//! out, three times in a row) means it is. An error that is not a drop proves nothing. Only then does `/v1/network-ready` answer 200, which
//! is what the pod's readiness probe reads, and the Operator claims no pod that is not Ready.
//!
//! The answer is kept once given: a policy attached to a pod stays attached.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// `host:port` of an address the pod's policy must refuse. Unset: nothing to prove, ready at once.
pub const ENV_POLICY_CANARY: &str = "ALIEN_SANDBOX_POLICY_CANARY";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const RETRY_EVERY: Duration = Duration::from_secs(1);

static ENFORCED: AtomicBool = AtomicBool::new(false);

/// Whether the pod's policy has been proven in force (or there is nothing to prove).
pub fn is_enforced() -> bool {
    ENFORCED.load(Ordering::Acquire)
}

/// Starts the proof in the background. Without a canary address the pod is ready at once.
pub fn start_from_env() {
    match std::env::var(ENV_POLICY_CANARY) {
        Ok(address) if !address.trim().is_empty() => {
            tokio::spawn(async move {
                prove_enforced(address.trim()).await;
                ENFORCED.store(true, Ordering::Release);
                tracing::info!("network policy enforcement proven for this pod");
            });
        }
        _ => ENFORCED.store(true, Ordering::Release),
    }
}

/// Returns once connections to `address` are dropped several times in a row. One timeout is not
/// proof: the Operator pod can be briefly unreachable or slow, and an error that is not a drop
/// (no route, network unreachable) says nothing about the policy, so it restarts the count.
pub async fn prove_enforced(address: &str) {
    let mut dropped = 0;
    loop {
        if dropped_by_policy(address).await {
            dropped += 1;
            if dropped >= REQUIRED_DROPS {
                return;
            }
        } else {
            dropped = 0;
        }
        tokio::time::sleep(RETRY_EVERY).await;
    }
}

/// Consecutive dropped connections that count as proof.
const REQUIRED_DROPS: u32 = 3;

/// Whether the connection to `address` timed out: a dropped packet. Accepted, reset and every
/// other error are not drops.
async fn dropped_by_policy(address: &str) -> bool {
    tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(address))
        .await
        .is_err()
}

/// Whether `address` answers: the connection is accepted, or the peer refuses it with a reset
/// (which only a reachable host sends).
#[cfg(test)]
async fn reachable(address: &str) -> bool {
    match tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(address)).await {
        Ok(Ok(_)) => true,
        Ok(Err(error)) => error.kind() == std::io::ErrorKind::ConnectionRefused,
        Err(_elapsed) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A listening port is reachable, so the policy is not in force.
    #[tokio::test]
    async fn an_address_that_accepts_is_reachable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        assert!(reachable(&listener.local_addr().unwrap().to_string()).await);
    }

    /// A reset comes from a reachable host that has nothing listening.
    #[tokio::test]
    async fn an_address_that_refuses_is_reachable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        drop(listener);
        assert!(reachable(&address).await);
    }

    /// The proof waits while the address answers and returns when it stops answering.
    #[tokio::test]
    async fn the_proof_waits_until_the_address_stops_answering() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let proof = tokio::spawn({
            let address = address.clone();
            async move { prove_enforced(&address).await }
        });
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(!proof.is_finished(), "still answering: not enforced");
        proof.abort();
    }

    /// An error that is not a drop (here an address that does not parse) never counts as proof.
    #[tokio::test]
    async fn an_error_that_is_not_a_drop_is_not_proof() {
        assert!(!dropped_by_policy("[::1]:1/invalid").await);
        tokio::time::timeout(Duration::from_secs(3), prove_enforced("[::1]:1/invalid"))
            .await
            .expect_err("never proven by a non-drop error");
    }
}
