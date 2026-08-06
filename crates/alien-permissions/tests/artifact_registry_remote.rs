use alien_permissions::get_permission_set;

#[test]
fn remote_registry_permission_is_data_only_on_every_cloud() {
    let permission = get_permission_set("artifact-registry/remote-read-write")
        .expect("remote registry permission set exists");

    let aws_actions = permission
        .platforms
        .aws
        .as_ref()
        .expect("AWS grants")
        .iter()
        .flat_map(|entry| entry.grant.actions.iter().flatten().map(String::as_str))
        .collect::<Vec<_>>();
    for required in [
        "ecr:GetAuthorizationToken",
        "ecr:BatchGetImage",
        "ecr:GetDownloadUrlForLayer",
        "ecr:InitiateLayerUpload",
        "ecr:UploadLayerPart",
        "ecr:CompleteLayerUpload",
        "ecr:PutImage",
    ] {
        assert!(
            aws_actions.contains(&required),
            "missing AWS OCI action {required}"
        );
    }
    for forbidden in [
        "ecr:CreateRepository",
        "ecr:DeleteRepository",
        "ecr:DeleteRepositoryPolicy",
        "ecr:SetRepositoryPolicy",
        "sts:AssumeRole",
    ] {
        assert!(
            !aws_actions.contains(&forbidden),
            "remote AWS data access must not grant {forbidden}"
        );
    }

    let gcp_entries = permission.platforms.gcp.as_ref().expect("GCP grants");
    let gcp_permissions = gcp_entries
        .iter()
        .flat_map(|entry| entry.grant.permissions.iter().flatten().map(String::as_str))
        .collect::<Vec<_>>();
    for required in [
        "artifactregistry.repositories.downloadArtifacts",
        "artifactregistry.repositories.uploadArtifacts",
    ] {
        assert!(
            gcp_permissions.contains(&required),
            "missing GCP OCI permission {required}"
        );
    }
    for forbidden in [
        "artifactregistry.repositories.create",
        "artifactregistry.repositories.delete",
        "artifactregistry.versions.delete",
        "iam.serviceAccounts.actAs",
        "iam.serviceAccounts.getAccessToken",
    ] {
        assert!(
            !gcp_permissions.contains(&forbidden),
            "remote GCP data access must not grant {forbidden}"
        );
    }
    assert!(
        gcp_entries
            .iter()
            .all(|entry| entry.grant.predefined_roles.is_none()),
        "a custom role avoids the broader, mutable Artifact Registry Writer role"
    );

    let azure_entries = permission.platforms.azure.as_ref().expect("Azure grants");
    let roles = azure_entries
        .iter()
        .flat_map(|entry| {
            entry
                .grant
                .predefined_roles
                .iter()
                .flatten()
                .map(String::as_str)
        })
        .collect::<Vec<_>>();
    assert_eq!(roles, vec!["AcrPush"]);
    assert!(!roles.contains(&"AcrDelete"));
}

#[test]
fn ordinary_push_no_longer_grants_repository_lifecycle() {
    let permission = get_permission_set("artifact-registry/push").expect("push permission exists");
    let serialized = serde_json::to_string(permission).expect("serialize permission set");
    for forbidden in [
        "ecr:CreateRepository",
        "ecr:DeleteRepository",
        "artifactregistry.repositories.create",
        "artifactregistry.repositories.delete",
        "artifactregistry.versions.delete",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "artifact-registry/push must not grant {forbidden}"
        );
    }
}
