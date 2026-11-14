//! The egress gateway's two VMs, owned by setup.
//!
//! Setup creates them once; a gateway image update rides a setup re-run. They sit in the
//! gateway subnet at fixed addresses (`.4` and `.5`, the ones the sandbox NSGs allow), have
//! no public IP, reach the Internet through the deployment NAT, and run the gateway image as
//! one hardened container. Their identity reads exactly two Key Vault secrets: the gateway
//! configuration and the replica secret. It never receives the signing key.

use alien_core::{
    import::EmitContext,
    sandbox_egress::{SANDBOX_EGRESS_GATEWAY_ID, SANDBOX_EGRESS_REPLICA_SECRET},
    Container, ContainerCode, ErrorData, Result, SECRETS_VAULT_ID,
};
use alien_error::AlienError;
use hcl::expr::Expression;

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    emitters::azure::{helpers::tags, sandbox_network::gateway_ip},
    expr,
};

/// The Key Vault secret holding the gateway configuration (see `alien-infra` `GATEWAY_CONFIG`).
pub(super) const CONFIG_SECRET: &str = "sandbox-egress-gateway-config";
/// Key Vault Secrets User.
const SECRETS_USER_ROLE: &str = "4633458b-17de-408a-b874-0445c86b69e6";
/// Host indices of the two gateway addresses inside the gateway subnet.
pub(super) const GATEWAY_HOSTS: [usize; 2] = [4, 5];

pub(super) fn emit(ctx: &EmitContext<'_>, network: &str, fragment: &mut TfFragment) -> Result<()> {
    let image = gateway_image(ctx)?;
    let vault = ctx.name_for(SECRETS_VAULT_ID).ok_or_else(|| {
        unsupported("the egress gateway reads its configuration from the secrets vault, which the stack does not declare")
    })?;
    let label = format!("{network}_sandbox_gateway");
    let subnet = expr::traversal(["azurerm_subnet", &label, "id"]);

    fragment.resource_blocks.push(resource_block(
        "azurerm_user_assigned_identity",
        &label,
        [
            attr(
                "name",
                expr::template(format!(
                    "${{local.resource_prefix}}-{network}-sandbox-gateway"
                )),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr("tags", tags(ctx, "sandbox-gateway-vm")),
        ],
    ));
    // Scoped to two secrets by name, so the identity cannot read the signing key.
    for secret in [CONFIG_SECRET, SANDBOX_EGRESS_REPLICA_SECRET] {
        let suffix = secret.replace('-', "_");
        fragment.resource_blocks.push(resource_block(
            "azurerm_role_assignment",
            &format!("{label}_{suffix}"),
            [
                attr("scope", expr::raw(format!("\"${{azurerm_key_vault.{vault}.id}}/secrets/{secret}\""))),
                attr(
                    "role_definition_id",
                    expr::raw(format!(
                        "\"/subscriptions/${{var.azure_subscription_id}}/providers/Microsoft.Authorization/roleDefinitions/{SECRETS_USER_ROLE}\""
                    )),
                ),
                attr("principal_id", expr::traversal(["azurerm_user_assigned_identity", &label, "principal_id"])),
                attr("principal_type", Expression::String("ServicePrincipal".into())),
            ],
        ));
    }

    fragment.resource_blocks.push(resource_block(
        "azurerm_availability_set",
        &label,
        [
            attr(
                "name",
                expr::template(format!(
                    "${{local.resource_prefix}}-{network}-sandbox-gateway"
                )),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr("platform_fault_domain_count", Expression::Number(2.into())),
            attr("tags", tags(ctx, "sandbox-gateway-vm")),
        ],
    ));

    let custom_data = cloud_config(&image, vault, &label);
    for (index, host) in GATEWAY_HOSTS.into_iter().enumerate() {
        let vm = format!("{label}_{index}");
        fragment.resource_blocks.push(resource_block(
            "azurerm_network_interface",
            &vm,
            [
                attr(
                    "name",
                    expr::template(format!(
                        "${{local.resource_prefix}}-{network}-sandbox-gateway-{index}"
                    )),
                ),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr("tags", tags(ctx, "sandbox-gateway-vm")),
                nested(block(
                    "ip_configuration",
                    [
                        attr("name", Expression::String("primary".into())),
                        attr("subnet_id", subnet.clone()),
                        attr(
                            "private_ip_address_allocation",
                            Expression::String("Static".into()),
                        ),
                        attr("private_ip_address", gateway_ip(network, host)),
                    ],
                )),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "azurerm_linux_virtual_machine",
            &vm,
            [
                attr(
                    "name",
                    expr::template(format!(
                        "${{local.resource_prefix}}-{network}-sandbox-gateway-{index}"
                    )),
                ),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                // The default computer name derives from the resource name, which has
                // underscores; Azure rejects them.
                attr(
                    "computer_name",
                    Expression::String(format!("alien-egress-gateway-{index}")),
                ),
                attr("size", Expression::String("Standard_B2s".into())),
                attr(
                    "availability_set_id",
                    expr::traversal(["azurerm_availability_set", &label, "id"]),
                ),
                attr(
                    "network_interface_ids",
                    Expression::Array(vec![expr::traversal([
                        "azurerm_network_interface",
                        &vm,
                        "id",
                    ])]),
                ),
                attr("admin_username", Expression::String("gateway".into())),
                // Nothing can log in: key-only auth with a key generated in state and never
                // output, no inbound 22 in the gateway subnet's NSG and no public IP. The VM is
                // administered through Azure (run command, redeploy).
                attr("disable_password_authentication", Expression::Bool(true)),
                nested(block(
                    "admin_ssh_key",
                    [
                        attr("username", Expression::String("gateway".into())),
                        attr(
                            "public_key",
                            expr::traversal(["tls_private_key", &label, "public_key_openssh"]),
                        ),
                    ],
                )),
                attr("custom_data", custom_data.clone()),
                attr(
                    "patch_mode",
                    Expression::String("AutomaticByPlatform".into()),
                ),
                attr(
                    "patch_assessment_mode",
                    Expression::String("AutomaticByPlatform".into()),
                ),
                attr("provision_vm_agent", Expression::Bool(true)),
                attr("encryption_at_host_enabled", Expression::Bool(false)),
                attr("tags", tags(ctx, "sandbox-gateway-vm")),
                nested(block(
                    "identity",
                    [
                        attr("type", Expression::String("UserAssigned".into())),
                        attr(
                            "identity_ids",
                            Expression::Array(vec![expr::traversal([
                                "azurerm_user_assigned_identity",
                                &label,
                                "id",
                            ])]),
                        ),
                    ],
                )),
                nested(block(
                    "os_disk",
                    [
                        attr("caching", Expression::String("ReadWrite".into())),
                        attr(
                            "storage_account_type",
                            Expression::String("Premium_LRS".into()),
                        ),
                    ],
                )),
                nested(block(
                    "source_image_reference",
                    [
                        attr("publisher", Expression::String("Canonical".into())),
                        attr("offer", Expression::String("ubuntu-24_04-lts".into())),
                        attr("sku", Expression::String("server".into())),
                        attr("version", Expression::String("latest".into())),
                    ],
                )),
            ],
        ));
    }
    fragment.resource_blocks.push(resource_block(
        "tls_private_key",
        &label,
        [attr("algorithm", Expression::String("ED25519".into()))],
    ));
    Ok(())
}

fn unsupported(reason: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationNotSupported {
        operation: "Azure sandbox gateway setup".into(),
        reason: reason.into(),
    })
}

fn gateway_image(ctx: &EmitContext<'_>) -> Result<String> {
    ctx.stack
        .resources()
        .find(|(id, _)| id.as_str() == SANDBOX_EGRESS_GATEWAY_ID)
        .and_then(|(_, entry)| entry.config.downcast_ref::<Container>())
        .and_then(|container| match &container.code {
            ContainerCode::Image { image } => Some(image.clone()),
            _ => None,
        })
        .ok_or_else(|| unsupported("the egress gateway container must name an image"))
}

/// Cloud-init: Docker, the gateway environment file and one systemd unit. The container runs
/// as a non-root user with no capabilities, a read-only root and `no-new-privileges`.
/// Values that come from Terraform are interpolated through `format`.
fn cloud_config(image: &str, vault: &str, label: &str) -> Expression {
    let lines = [
        "#cloud-config",
        "package_update: true",
        "packages: [docker.io]",
        "write_files:",
        "  - path: /etc/alien-egress-gateway.env",
        "    permissions: '0600'",
        "    content: |",
        "      ALIEN_DEPLOYMENT_TYPE=azure",
        "      AZURE_CLIENT_ID=%s",
        "      AZURE_SUBSCRIPTION_ID=%s",
        "      AZURE_TENANT_ID=%s",
        "      ALIEN_SECRETS_BINDING=%s",
        "  - path: /etc/systemd/system/alien-egress-gateway.service",
        "    content: |",
        "      [Unit]",
        "      Description=Alien egress gateway",
        "      After=docker.service network-online.target",
        "      Requires=docker.service",
        "      [Service]",
        "      Restart=always",
        "      RestartSec=3",
        "      ExecStartPre=-/usr/bin/docker rm -f gateway",
        &format!("      ExecStart=/usr/bin/docker run --name gateway --env-file /etc/alien-egress-gateway.env --user 1000:1000 --cap-drop ALL --security-opt no-new-privileges --read-only --tmpfs /tmp -p 3128:3128 -p 3129:3129 {image}"),
        "      [Install]",
        "      WantedBy=multi-user.target",
        "runcmd:",
        "  - [systemctl, daemon-reload]",
        "  - [systemctl, enable, --now, alien-egress-gateway.service]",
        "",
    ];
    let body = lines
        .join("\n")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace("${", "$${");
    expr::raw(format!(
        "base64encode(format(\"{body}\", azurerm_user_assigned_identity.{label}.client_id, var.azure_subscription_id, data.azurerm_client_config.{vault}_current.tenant_id, jsonencode({{service = \"key-vault\", vaultName = azurerm_key_vault.{vault}.name}})))"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered() -> String {
        let body = hcl::Body::builder()
            .add_attribute((
                "custom_data",
                cloud_config(
                    "registry.example/gw@sha256:abc",
                    "secrets",
                    "net_sandbox_gateway",
                ),
            ))
            .build();
        hcl::to_string(&body).unwrap()
    }

    #[test]
    fn the_container_is_unprivileged_and_pinned() {
        let text = rendered();
        for needed in [
            "--cap-drop ALL",
            "no-new-privileges",
            "--read-only",
            "--user 1000:1000",
            "registry.example/gw@sha256:abc",
        ] {
            assert!(text.contains(needed), "{needed} missing:\n{text}");
        }
        assert!(!text.contains("--network host") && !text.contains("--privileged"));
    }

    #[test]
    fn the_four_terraform_values_fill_the_four_placeholders() {
        let text = rendered();
        assert_eq!(text.matches("%s").count(), 4, "{text}");
        // jsonencode keeps the vault binding valid JSON whatever the vault name holds.
        assert!(text.contains("jsonencode"));
    }

    #[test]
    fn the_vm_takes_a_key_and_no_password() {
        let src = include_str!("sandbox_gateway_vm.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        assert!(code.contains("tls_private_key") && code.contains("admin_ssh_key"));
        assert!(!code.contains("random_password") && !code.contains("\"admin_password\""));
    }

    #[test]
    fn the_identity_reads_only_the_config_and_replica_secrets() {
        assert_eq!(CONFIG_SECRET, "sandbox-egress-gateway-config");
        assert_ne!(SANDBOX_EGRESS_REPLICA_SECRET, "sandbox-egress-signing-key");
        assert_eq!(GATEWAY_HOSTS, [4, 5]);
        // Key Vault Secrets User, a built-in role Azure defines.
        assert_eq!(SECRETS_USER_ROLE, "4633458b-17de-408a-b874-0445c86b69e6");
    }
}
