use alien_platform_api::{types, Client};

#[test]
fn generated_client_exposes_external_id_operations() {
    let client = Client::new("https://api.alien.dev");
    let body: types::EnsureDeploymentGroupByExternalIdRequest =
        types::EnsureDeploymentGroupByExternalIdRequest::builder()
            .external_id("customer_123")
            .name("production")
            .project("my-project")
            .try_into()
            .expect("valid ensure request");

    // Construct both generated request builders. Calling `send` is covered by
    // API integration tests; this test protects the public Rust SDK surface.
    let _ensure = client.ensure_deployment_group_by_external_id().body(body);
    let _read = client
        .get_deployment_group_by_external_id()
        .external_id("customer_123")
        .project("my-project");
    let update: types::SetDeploymentGroupExternalIdRequest =
        types::SetDeploymentGroupExternalIdRequest::builder()
            .external_id(Some(
                "customer_456"
                    .parse::<types::SetDeploymentGroupExternalIdRequestExternalId>()
                    .expect("valid external ID"),
            ))
            .try_into()
            .expect("valid update request");
    let _update = client
        .set_deployment_group_external_id()
        .id("dg_0000000000000000000000000000")
        .body(update);
}
