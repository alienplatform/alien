//! Release channels on the SQLite stores: which deployments each release
//! reaches as channels advance, deployments move between channels, and pins
//! hold a deployment on one release.

use std::collections::HashMap;
use std::sync::Arc;

use alien_core::{Platform, StackSettings};
use alien_manager::auth::{Role, Scope, Subject, SubjectKind};
use alien_manager::stores::sqlite::{
    SqliteDatabase, SqliteDeploymentStore, SqliteReleaseChannelStore, SqliteReleaseStore,
};
use alien_manager::traits::deployment_store::*;
use alien_manager::traits::release_store::*;
use alien_manager::traits::{DeploymentRouting, ReleaseChannelStore, DEFAULT_CHANNEL};

fn admin() -> Subject {
    Subject {
        kind: SubjectKind::ServiceAccount {
            id: "test".to_string(),
        },
        workspace_id: "default".to_string(),
        scope: Scope::Workspace,
        role: Role::WorkspaceAdmin,
        bearer_token: String::new(),
    }
}

struct Fixture {
    db: Arc<SqliteDatabase>,
    deployments: SqliteDeploymentStore,
    releases: SqliteReleaseStore,
    channels: SqliteReleaseChannelStore,
    group_id: String,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manager.db");
        std::mem::forget(dir);
        let db = Arc::new(SqliteDatabase::new(path.to_str().unwrap()).await.unwrap());
        let deployments = SqliteDeploymentStore::new(db.clone());
        let group_id = deployments
            .create_deployment_group(
                &admin(),
                CreateDeploymentGroupParams {
                    name: "customers".to_string(),
                    max_deployments: 100,
                    setup: Default::default(),
                },
            )
            .await
            .unwrap()
            .id;
        Self {
            releases: SqliteReleaseStore::new(db.clone()),
            channels: SqliteReleaseChannelStore::new(db.clone()),
            deployments,
            db,
            group_id,
        }
    }

    /// A deployment that has converged and can take updates.
    async fn running_deployment(&self, name: &str) -> String {
        let deployment = self
            .deployments
            .create_deployment(
                &admin(),
                CreateDeploymentParams {
                    deployment_protocol_version: alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
                    name: name.to_string(),
                    deployment_group_id: self.group_id.clone(),
                    platform: Platform::Kubernetes,
                    base_platform: None,
                    stack_settings: StackSettings::default(),
                    stack_state: None,
                    environment_variables: None,
                    public_subdomain: None,
                    input_values: HashMap::new(),
                    setup_item: None,
                    deployment_token: None,
                },
            )
            .await
            .unwrap();
        self.settle(&deployment.id).await;
        deployment.id
    }

    /// Mark a deployment as having converged on its desired release.
    async fn settle(&self, id: &str) {
        let conn = self.db.conn().lock().await;
        conn.execute(
            "UPDATE deployments SET status = 'running', current_release_id = desired_release_id WHERE id = ?1",
            (id,),
        )
        .await
        .unwrap();
    }

    async fn release(&self) -> String {
        self.releases
            .create_release(
                &admin(),
                CreateReleaseParams {
                    project_id: "default".to_string(),
                    stacks: HashMap::from([(
                        Platform::Kubernetes,
                        alien_core::Stack::new("app".to_string()).build(),
                    )]),
                    git_commit_sha: None,
                    git_commit_ref: None,
                    git_commit_message: None,
                },
            )
            .await
            .unwrap()
            .id
    }

    /// Publish a release to a channel, as `POST /v1/releases` does.
    async fn publish(&self, channel: &str) -> String {
        let id = self.release().await;
        self.channels
            .set_channel_release(channel, &id)
            .await
            .unwrap();
        self.channels.roll_out_channel(channel, &id).await.unwrap();
        id
    }

    async fn desired(&self, id: &str) -> (Option<String>, String) {
        let deployment = self
            .deployments
            .get_deployment(&admin(), id)
            .await
            .unwrap()
            .unwrap();
        (deployment.desired_release_id, deployment.status)
    }
}

#[tokio::test]
async fn releases_follow_channels_and_pins() {
    let f = Fixture::new().await;
    let a = f.running_deployment("a").await;
    let b = f.running_deployment("b").await;
    let c = f.running_deployment("c").await;

    // Everything follows production until told otherwise.
    assert_eq!(
        f.channels.deployment_routing(&a).await.unwrap(),
        DeploymentRouting {
            channel: DEFAULT_CHANNEL.to_string(),
            pinned_release_id: None
        }
    );
    f.channels.create_channel("staging", None).await.unwrap();
    f.channels
        .create_channel("staging", None)
        .await
        .expect_err("a channel can't be created twice");
    f.channels
        .set_deployment_routing(
            &b,
            &DeploymentRouting {
                channel: "staging".to_string(),
                pinned_release_id: None,
            },
        )
        .await
        .unwrap();

    // A production release reaches a and c, not b.
    let r1 = f.publish(DEFAULT_CHANNEL).await;
    assert_eq!(
        f.desired(&a).await,
        (Some(r1.clone()), "update-pending".to_string())
    );
    assert_eq!(
        f.desired(&c).await,
        (Some(r1.clone()), "update-pending".to_string())
    );
    assert_eq!(f.desired(&b).await.0, None);
    for id in [&a, &b, &c] {
        f.settle(id).await;
    }

    // Pin c; the next production release passes it by.
    f.channels
        .set_deployment_routing(
            &c,
            &DeploymentRouting {
                channel: DEFAULT_CHANNEL.to_string(),
                pinned_release_id: Some(r1.clone()),
            },
        )
        .await
        .unwrap();
    let r2 = f.publish(DEFAULT_CHANNEL).await;
    assert_eq!(f.desired(&a).await.0.as_deref(), Some(r2.as_str()));
    assert_eq!(
        f.desired(&c).await,
        (Some(r1.clone()), "running".to_string())
    );

    // Promoting r2 to staging reaches b.
    f.channels
        .set_channel_release("staging", &r2)
        .await
        .unwrap();
    f.channels.roll_out_channel("staging", &r2).await.unwrap();
    assert_eq!(
        f.desired(&b).await,
        (Some(r2.clone()), "update-pending".to_string())
    );

    // Unpinning c sends it production's release.
    f.channels
        .set_deployment_routing(
            &c,
            &DeploymentRouting {
                channel: DEFAULT_CHANNEL.to_string(),
                pinned_release_id: None,
            },
        )
        .await
        .unwrap();
    f.channels.roll_out_deployment(&c, &r2).await.unwrap();
    assert_eq!(
        f.desired(&c).await,
        (Some(r2.clone()), "update-pending".to_string())
    );

    assert_eq!(
        f.channels.count_following(DEFAULT_CHANNEL).await.unwrap(),
        2
    );
    assert_eq!(f.channels.count_following("staging").await.unwrap(), 1);
    let channels = f.channels.list_channels().await.unwrap();
    let names: Vec<_> = channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["production", "staging"]);
    assert_eq!(channels[0].current_release_id.as_deref(), Some(r2.as_str()));
    assert!(f.channels.delete_channel("staging").await.unwrap());
    assert!(!f.channels.delete_channel("staging").await.unwrap());
}

#[tokio::test]
async fn deployments_mid_rollout_are_left_alone() {
    let f = Fixture::new().await;
    let a = f.running_deployment("a").await;
    {
        let conn = f.db.conn().lock().await;
        conn.execute(
            "UPDATE deployments SET status = 'update-pending' WHERE id = ?1",
            (a.as_str(),),
        )
        .await
        .unwrap();
    }
    let r1 = f.publish(DEFAULT_CHANNEL).await;
    // Same rule as releases without channels: only settled deployments take
    // a new release; the channel still records it for later.
    assert_eq!(f.desired(&a).await.0, None);
    assert_eq!(
        f.channels
            .get_channel(DEFAULT_CHANNEL)
            .await
            .unwrap()
            .unwrap()
            .current_release_id,
        Some(r1)
    );
}
