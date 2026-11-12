//! Proof that the cluster is enforcing this pod's NetworkPolicy before the pod takes a session.
//!
//! On a fresh node the network policy agent can lag behind the first pods scheduled onto it, and
//! a pod that starts in that window has unrestricted egress until its policy is loaded. The
//! Operator therefore names an address the policy must refuse (its own pod address) and the
//! agent tries to connect to it. A connection that is accepted or refused outright means the
//! pod can reach it, so the policy is not in force yet. A connection that is dropped (it times
//! out) or cannot be routed means it is. Only then does `/v1/network-ready` answer 200, which
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

/// Returns once a connection to `address` is dropped or cannot be routed.
pub async fn prove_enforced(address: &str) {
    loop {
        if !reachable(address).await {
            return;
        }
        tokio::time::sleep(RETRY_EVERY).await;
    }
}

/// Whether `address` answers: the connection is accepted, or the peer refuses it with a reset
/// (which only a reachable host sends). A timeout is a dropped packet; any other error means the
/// network refused to carry it.
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
        // An unroutable address stands in for a dropped packet.
        proof.abort();
        tokio::time::timeout(Duration::from_secs(5), prove_enforced("[::1]:1/invalid"))
            .await
            .expect("an address that cannot be reached counts as blocked");
    }
}
