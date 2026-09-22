use super::*;

const TEST_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const WRONG_KEY: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

#[tokio::test]
async fn generated_key_encrypts_database_and_survives_reopen() {
    let directory = tempfile::tempdir().expect("create temporary directory");
    let path = directory.path().join("manager.db");
    let sentinel = "manager-encryption-sentinel-generated";

    {
        let database = SqliteDatabase::new(path.to_str().expect("utf-8 path"))
            .await
            .expect("create encrypted database");
        database
            .execute("CREATE TABLE secure_test (value TEXT NOT NULL)")
            .await
            .expect("create test table");
        database
            .conn()
            .lock()
            .await
            .execute("INSERT INTO secure_test VALUES (?)", (sentinel,))
            .await
            .expect("insert sentinel");
        assert_no_plaintext_in_database_files(&path, sentinel);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(key_path(&path))
                .expect("read generated key metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    let reopened = SqliteDatabase::new(path.to_str().expect("utf-8 path"))
        .await
        .expect("reopen with persisted generated key");
    let connection = reopened.conn().lock().await;
    assert_eq!(
        query_i64(&connection, "SELECT COUNT(*) FROM secure_test")
            .await
            .expect("count migrated rows"),
        1
    );
    drop(connection);
    assert_no_plaintext_in_database_files(&path, sentinel);
}

#[tokio::test]
async fn missing_generated_key_never_replaces_key_for_encrypted_state() {
    let directory = tempfile::tempdir().expect("create temporary directory");
    let path = directory.path().join("manager.db");
    let database = SqliteDatabase::new(path.to_str().expect("utf-8 path"))
        .await
        .expect("create encrypted database");
    drop(database);
    std::fs::remove_file(key_path(&path)).expect("remove generated key");

    let error = SqliteDatabase::new(path.to_str().expect("utf-8 path"))
        .await
        .err()
        .expect("missing key must fail");
    assert!(error.to_string().contains("restore the original key"));
    assert!(
        !key_path(&path).exists(),
        "must not create a replacement key"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_first_open_uses_one_key_and_one_database() {
    let directory = tempfile::tempdir().expect("create temporary directory");
    let path = directory.path().join("manager.db");
    let first_path = path.clone();
    let second_path = path.clone();
    let first =
        tokio::spawn(
            async move { SqliteDatabase::new(first_path.to_str().expect("utf-8 path")).await },
        );
    let second = tokio::spawn(async move {
        SqliteDatabase::new(second_path.to_str().expect("utf-8 path")).await
    });
    drop(first.await.expect("first task").expect("first open"));
    drop(second.await.expect("second task").expect("second open"));

    SqliteDatabase::new(path.to_str().expect("utf-8 path"))
        .await
        .expect("reopen with the winning key");
}

#[tokio::test]
async fn plaintext_database_is_migrated_without_losing_rows() {
    let directory = tempfile::tempdir().expect("create temporary directory");
    let path = directory.path().join("manager.db");
    let sentinel = "manager-encryption-sentinel-migrated";
    create_plaintext_database(&path, sentinel).await;
    assert!(file_contains(&path, sentinel.as_bytes()));

    let database = SqliteDatabase::new_with_key(path.to_str().expect("utf-8 path"), Some(TEST_KEY))
        .await
        .expect("migrate plaintext database");
    let mut rows = database
        .conn()
        .lock()
        .await
        .query("SELECT value FROM secure_test", ())
        .await
        .expect("query migrated row");
    let row = rows.next().await.expect("read row").expect("row exists");
    assert_eq!(row.get::<String>(0).expect("read value"), sentinel);
    assert!(!sibling_path(&path, BACKUP_SUFFIX).exists());
    assert_no_plaintext_in_database_files(&path, sentinel);

    drop(rows);
    drop(database);
    assert!(
        SqliteDatabase::new_with_key(path.to_str().expect("utf-8 path"), Some(WRONG_KEY))
            .await
            .is_err(),
        "wrong key must not open encrypted state"
    );
}

#[tokio::test]
async fn interrupted_promotion_resumes_from_valid_encrypted_copy() {
    let directory = tempfile::tempdir().expect("create temporary directory");
    let path = directory.path().join("manager.db");
    let staging = sibling_path(&path, STAGING_SUFFIX);
    let backup = sibling_path(&path, BACKUP_SUFFIX);
    let sentinel = "manager-encryption-sentinel-recovery";
    create_plaintext_database(&path, sentinel).await;
    migrate_plaintext_database(&path, &staging, TEST_KEY)
        .await
        .expect("build encrypted staging database");
    std::fs::rename(&path, &backup).expect("simulate crash after preserving source");

    let database = SqliteDatabase::new_with_key(path.to_str().expect("utf-8 path"), Some(TEST_KEY))
        .await
        .expect("resume interrupted promotion");
    assert!(path.exists());
    assert!(!staging.exists());
    assert!(!backup.exists());
    let mut rows = database
        .conn()
        .lock()
        .await
        .query("SELECT value FROM secure_test", ())
        .await
        .expect("query recovered row");
    let row = rows.next().await.expect("read row").expect("row exists");
    assert_eq!(row.get::<String>(0).expect("read value"), sentinel);
}

async fn create_plaintext_database(path: &Path, sentinel: &str) {
    let database = turso::Builder::new_local(&path.to_string_lossy())
        .build()
        .await
        .expect("create plaintext database");
    let connection = database.connect().expect("connect plaintext database");
    let wrapper = SqliteDatabase {
        conn: Arc::new(Mutex::new(connection.clone())),
    };
    migrations::run_migrations(&wrapper)
        .await
        .expect("create current manager schema");
    connection
        .execute("CREATE TABLE secure_test (value TEXT NOT NULL)", ())
        .await
        .expect("create plaintext table");
    connection
        .execute("INSERT INTO secure_test VALUES (?)", (sentinel,))
        .await
        .expect("insert plaintext row");
    consume_query(&connection, "PRAGMA wal_checkpoint(TRUNCATE)")
        .await
        .expect("checkpoint plaintext database");
}

fn assert_no_plaintext_in_database_files(path: &Path, sentinel: &str) {
    for candidate in [
        path.to_path_buf(),
        sibling_path(path, "-wal"),
        sibling_path(path, "-shm"),
        sibling_path(path, "-log"),
    ] {
        if candidate.exists() {
            assert!(
                !file_contains(&candidate, sentinel.as_bytes()),
                "{} contains plaintext sentinel",
                candidate.display()
            );
        }
    }
}

fn file_contains(path: &Path, needle: &[u8]) -> bool {
    std::fs::read(path)
        .expect("read database artifact")
        .windows(needle.len())
        .any(|window| window == needle)
}
