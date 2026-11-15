//! Every setup-key mint leaves an audit event with ids and times, never the key.
//! Its own test binary: a captured log needs a subscriber no other test touches.

use std::{
    io::Write,
    sync::{Arc, Mutex},
};

use alien_manager::{
    stores::sqlite::{SqliteDatabase, SqliteTokenStore},
    traits::token_store::{TokenStore, TokenType},
};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn hash(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

#[tokio::test]
async fn a_mint_is_audited_without_key_material() {
    let sink = Sink::default();
    let writer = sink.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish(),
    )
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        SqliteDatabase::new(&dir.path().join("manager.db").to_string_lossy())
            .await
            .unwrap(),
    );
    let tokens = SqliteTokenStore::new(db);
    let (raw_one, raw_two) = ("ax_setup_RAWSECRETONE", "ax_setup_RAWSECRETTWO");
    let first = tokens
        .create_setup_token(
            TokenType::Setup,
            "dep-1",
            "ax_setup_one",
            &hash(raw_one),
            Utc::now() + Duration::minutes(15),
        )
        .await
        .unwrap();
    let second = tokens
        .create_setup_token(
            TokenType::Setup,
            "dep-1",
            "ax_setup_two",
            &hash(raw_two),
            Utc::now() + Duration::minutes(15),
        )
        .await
        .unwrap();
    let imported = tokens
        .create_setup_token(
            TokenType::SetupImport,
            "dep-1",
            "ax_setup_three",
            &hash("ax_setup_RAWSECRETTHREE"),
            Utc::now() + Duration::minutes(15),
        )
        .await
        .unwrap();

    let log = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert_eq!(log.matches("setup_key_minted").count(), 3, "{log}");
    for needle in ["dep-1", "setup-import", "ax_setup_two", &second.id, &imported.id] {
        assert!(log.contains(needle), "missing {needle}: {log}");
    }
    // The second Operator mint names the one it revoked; the import mint
    // revokes none of the Operator's.
    let line = |id: &str| log.lines().find(|l| l.contains(&format!("token_id={id}"))).unwrap();
    assert!(line(&second.id).contains(&first.id), "{log}");
    assert!(!line(&imported.id).contains(&second.id), "{log}");
    for secret in [raw_one, raw_two, "RAWSECRETTHREE", &hash(raw_one), &hash(raw_two)] {
        assert!(!log.contains(secret), "key material in the log");
    }
}
