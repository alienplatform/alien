//! The names setup gives workload identities. Expected values are what
//! `terraform console` returns for the Terraform setup's own expressions
//! (`iam_role_name_template` and `service_account_id_template`).

use alien_signing::settings::names::{aws_workload_roles, gcp_workload_service_accounts};

#[test]
fn aws_roles_match_setup() {
    assert_eq!(
        aws_workload_roles("shop-prod", "api-runtime"),
        ["shop-prod-api-runtime-sa"]
    );
    assert_eq!(
        aws_workload_roles(
            "a-very-long-install-prefix-that-goes-past-the-limit",
            "api-runtime"
        ),
        ["a-very-long-install-prefix-that-goes-past-the-limit-api-a0c8f96c"]
    );
}

#[test]
fn gcp_service_accounts_match_setup() {
    assert_eq!(
        gcp_workload_service_accounts("shop-prod", "api-runtime"),
        ["shop-prod-api-runtime-sa", "a-shop-prod-api-runti-603fe27e"]
    );
    assert_eq!(
        gcp_workload_service_accounts("Shop_Prod", "api-runtime"),
        ["Shop_Prod-api-runtime-sa", "a-shop-prod-api-runti-603fe27e"]
    );
    assert_eq!(
        gcp_workload_service_accounts(
            "a-very-long-install-prefix-that-goes-past-the-limit",
            "api-runtime"
        )[1],
        "a-a-very-long-install-a0c8f96c"
    );
}
