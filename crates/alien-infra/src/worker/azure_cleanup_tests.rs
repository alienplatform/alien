use super::commands_queue_cleanup_target;

#[test]
fn partial_commands_queue_cleanup_state_fails_closed() {
    assert!(commands_queue_cleanup_target(
        Some("rg".to_string()),
        Some("namespace".to_string()),
        None,
        "worker"
    )
    .is_err());
    assert!(commands_queue_cleanup_target(
        None,
        Some("namespace".to_string()),
        Some("queue".to_string()),
        "worker"
    )
    .is_err());
    assert_eq!(
        commands_queue_cleanup_target(None, None, None, "worker").unwrap(),
        None
    );
}
