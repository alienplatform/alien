use alien_cli::commands::{
    commands::{CommandsAction, CommandsArgs},
    deployments::{DeploymentsArgs, DeploymentsCmd},
    destroy::DestroyArgs,
    logs::LogsArgs,
};
use clap::Parser;
use std::time::Duration;

#[test]
fn wait_accepts_seconds_and_units_but_rejects_zero() {
    for (input, seconds) in [("580", 580), ("2m", 120), ("1h", 3600)] {
        let args = DeploymentsArgs::try_parse_from([
            "deployments",
            "wait",
            "dep_test",
            "--timeout",
            input,
        ])
        .unwrap();
        let DeploymentsCmd::Wait { timeout, .. } = args.cmd else {
            panic!("expected wait")
        };
        assert_eq!(timeout, Duration::from_secs(seconds));
    }
    assert!(
        DeploymentsArgs::try_parse_from(["deployments", "wait", "dep_test", "--timeout", "0"])
            .is_err()
    );
}

#[test]
fn date_only_log_bounds_mean_midnight_utc() {
    let args = LogsArgs::try_parse_from([
        "logs",
        "--from",
        "2026-10-24",
        "--to",
        "2026-10-25T03:00:00+03:00",
    ])
    .unwrap();
    assert_eq!(args.from.unwrap().to_rfc3339(), "2026-10-24T00:00:00+00:00");
    assert_eq!(args.to.unwrap().to_rfc3339(), "2026-10-25T00:00:00+00:00");
    assert!(LogsArgs::try_parse_from(["logs", "--from", "2026-02-30"]).is_err());
}

#[test]
fn command_target_and_destroy_confirmation_are_explicit() {
    let args = CommandsArgs::try_parse_from([
        "commands",
        "invoke",
        "--deployment",
        "dep_test",
        "--command",
        "ping",
        "--target",
        "worker-a",
    ])
    .unwrap();
    let CommandsAction::Invoke { target, .. } = args.action;
    assert_eq!(target.as_deref(), Some("worker-a"));
    let args = DestroyArgs::try_parse_from([
        "destroy",
        "--name",
        "dep_test",
        "--platform",
        "machines",
        "--yes",
    ])
    .unwrap();
    assert!(args.yes);
    assert!(!args.force);
}
