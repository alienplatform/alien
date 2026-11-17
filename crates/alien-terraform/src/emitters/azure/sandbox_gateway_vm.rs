//! The egress gateway's two VMs, owned by setup.
//!
//! Setup creates them once; a gateway image update rides a setup re-run, one VM at a time (a
//! run command on each VM installs the image, and the second waits for the first). They sit in the
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

/// The Ubuntu 24.04 LTS server image version the gateway VMs are created from. Pinned to a version
/// that a real apply created VMs from: Azure's image listing can show a newer version
/// (24.04.202611050 was listed) that a create refuses with `PlatformImageNotFound`, so check a bump
/// with `az vm image show --urn Canonical:ubuntu-24_04-lts:server:<version>` in the target regions.
pub(super) const UBUNTU_IMAGE_VERSION: &str = "24.04.202610250";

/// A general-purpose size that most subscriptions allow; burstable B-series sizes are
/// restricted on some, and vCPU quota is per family, so customers can pick another family.
pub(super) const DEFAULT_VM_SIZE: &str = "Standard_D2s_v5";

fn vm_size_variable(label: &str) -> hcl::Block {
    hcl::Block::builder("variable")
        .add_label(format!("{label}_vm_size"))
        .add_attribute(("type", expr::raw("string")))
        .add_attribute(("default", Expression::String(DEFAULT_VM_SIZE.into())))
        .add_attribute((
            "description",
            Expression::String(
                "Azure VM size of the two egress gateway VMs. Use a size with at least 2 vCPUs and 4 GiB of memory, \
                 available to your subscription in this region and with vCPU quota for two VMs of its family."
                    .into(),
            ),
        ))
        .add_block(
            hcl::Block::builder("validation")
                .add_attribute((
                    "condition",
                    expr::raw(format!("can(regex(\"^Standard_[A-Za-z0-9_]+$\", var.{label}_vm_size))")),
                ))
                .add_attribute((
                    "error_message",
                    Expression::String("The gateway VM size must be an Azure size name such as Standard_D2s_v5.".into()),
                ))
                .build(),
        )
        .build()
}

pub(super) fn emit(ctx: &EmitContext<'_>, network: &str, fragment: &mut TfFragment) -> Result<()> {
    let image = gateway_image(ctx)?;
    let vault = ctx.name_for(SECRETS_VAULT_ID).ok_or_else(|| {
        unsupported("the egress gateway reads its configuration from the secrets vault, which the stack does not declare")
    })?;
    let label = format!("{network}_sandbox_gateway");
    let subnet = expr::traversal(["azurerm_subnet", &label, "id"]);
    fragment.variable_blocks.push(vm_size_variable(&label));

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

    let custom_data = cloud_config();
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
                attr("size", expr::raw(format!("var.{label}_vm_size"))),
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
                // azurerm requires an SSH key when the password is disabled. The generated key
                // lives in state, but nothing can use it: there is no inbound 22 and no public IP.
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
                // Off on purpose: host encryption needs the subscription's
                // `Microsoft.Compute/EncryptionAtHost` feature registered, and the apply fails
                // without it. The OS disk is still encrypted at rest with platform-managed keys,
                // and the VM holds no customer data (policies live in memory, the image is public).
                attr("encryption_at_host_enabled", Expression::Bool(false)),
                attr("tags", tags(ctx, "sandbox-gateway-vm")),
                // The image never rides custom_data: an Azure VM replaces itself when custom_data
                // changes, which would take both gateways down together. Installs from before the
                // run command carried the image in custom_data; their change must not replace.
                nested(block(
                    "lifecycle",
                    [attr("ignore_changes", expr::raw("[custom_data]"))],
                )),
                // First boot needs outbound access (apt, the image pull): wait for the subnet's
                // NAT and NSG associations, which nothing else in the VM refers to.
                attr(
                    "depends_on",
                    expr::raw(format!(
                        "[azurerm_subnet_nat_gateway_association.{label}, azurerm_subnet_network_security_group_association.{label}]"
                    )),
                ),
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
                        // Pinned: "latest" would give a VM replacement a different OS than the
                        // running one. Patches arrive through `patch_mode` (platform-managed).
                        attr("version", Expression::String(UBUNTU_IMAGE_VERSION.into())),
                    ],
                )),
            ],
        ));
    }
    // One run command per VM installs the image, the environment file and the unit, restarts the
    // gateway and, on a VM that was serving, waits until the new container is ready and has caught
    // up from its peer. The second VM's command depends on the first, so a setup re-run never has
    // both gateways down.
    for index in 0..GATEWAY_HOSTS.len() {
        let vm = format!("{label}_{index}");
        let peer_host = GATEWAY_HOSTS[1 - index];
        let mut depends = vec![
            format!(
                "azurerm_role_assignment.{label}_{}",
                CONFIG_SECRET.replace('-', "_")
            ),
            format!(
                "azurerm_role_assignment.{label}_{}",
                SANDBOX_EGRESS_REPLICA_SECRET.replace('-', "_")
            ),
        ];
        if index > 0 {
            depends.push(format!(
                "azurerm_virtual_machine_run_command.{label}_{}",
                index - 1
            ));
        }
        fragment.resource_blocks.push(resource_block(
            "azurerm_virtual_machine_run_command",
            &vm,
            [
                attr("name", Expression::String("install-gateway".into())),
                attr("location", expr::raw("var.azure_location")),
                attr(
                    "virtual_machine_id",
                    expr::traversal(["azurerm_linux_virtual_machine", &vm, "id"]),
                ),
                nested(block(
                    "source",
                    [attr(
                        "script",
                        install_script(&image, vault, &label, network, peer_host),
                    )],
                )),
                attr("tags", tags(ctx, "sandbox-gateway-vm")),
                attr("depends_on", expr::raw(format!("[{}]", depends.join(", ")))),
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

/// First boot only: install Docker. Nothing else rides custom_data (see the `lifecycle` block).
fn cloud_config() -> Expression {
    let lines = [
        "#cloud-config",
        "runcmd:",
        // The first boot can run before the subnet's NAT association finishes, so apt may have no
        // route yet: retry instead of giving up once.
        "  - [bash, -c, 'for i in $(seq 1 30); do apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y docker.io && exit 0; sleep 10; done; exit 1']",
        "",
    ];
    let body = hcl_string(&lines.join("\n"));
    expr::raw(format!("base64encode(\"{body}\")"))
}

/// Escapes text for an HCL quoted string.
fn hcl_string(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace("${", "$${")
        .replace("%{", "%%{")
}

/// The per-VM run command. It pulls the image before it touches the running gateway, writes the
/// environment file and the unit (a hardened, unprivileged container: no capabilities, a read-only
/// root, `no-new-privileges`), restarts the gateway and, when this VM was serving, refuses to start
/// if the peer is not serving (never restart the only serving gateway) and waits until the new
/// container is ready, meaning it has caught up its policies from the peer. The gateway opens its
/// proxy port only once ready, so a new image that never catches up leaves its port closed and
/// agents fail over to the peer: a failed wait (exit 1, which fails the apply and stops the roll)
/// needs no rollback to keep egress working. A fresh VM has no
/// configuration to serve yet (setup publishes it afterwards), so it does not wait.
/// Values that come from Terraform are interpolated through `format`.
fn install_script(
    image: &str,
    vault: &str,
    label: &str,
    network: &str,
    peer_host: usize,
) -> Expression {
    let lines = [
        "#!/bin/bash",
        "set -u",
        "cloud-init status --wait >/dev/null 2>&1 || true",
        "health() { curl -sf -m 3 -o /dev/null \"http://$1:3129/health\"; }",
        "serving=no",
        "health 127.0.0.1 && serving=yes",
        "if [ \"$serving\" = yes ] && ! health %s; then",
        "  echo 'the peer gateway is not serving; not restarting the only serving gateway' >&2",
        "  exit 1",
        "fi",
        &format!("docker pull {image} || exit 1"),
        "umask 077",
        "cat > /etc/alien-egress-gateway.env <<'ENV'",
        "ALIEN_DEPLOYMENT_TYPE=azure",
        "AZURE_CLIENT_ID=%s",
        "AZURE_SUBSCRIPTION_ID=%s",
        "AZURE_TENANT_ID=%s",
        "ALIEN_SECRETS_BINDING=%s",
        "RUST_LOG=warn",
        "ENV",
        "cat > /etc/systemd/system/alien-egress-gateway.service <<'UNIT'",
        "[Unit]",
        "Description=Alien egress gateway",
        "After=docker.service network-online.target",
        "Requires=docker.service",
        "[Service]",
        "Restart=always",
        "RestartSec=3",
        "ExecStartPre=-/usr/bin/docker rm -f gateway",
        &format!("ExecStart=/usr/bin/docker run --name gateway --env-file /etc/alien-egress-gateway.env --user 1000:1000 --cap-drop ALL --memory 2g --security-opt no-new-privileges --read-only --tmpfs /tmp -p 3128:3128 -p 3129:3129 {image}"),
        "[Install]",
        "WantedBy=multi-user.target",
        "UNIT",
        "systemctl daemon-reload",
        "systemctl enable alien-egress-gateway.service",
        "systemctl restart alien-egress-gateway.service",
        "[ \"$serving\" = yes ] || exit 0",
        "for i in $(seq 1 60); do health 127.0.0.1 && exit 0; sleep 5; done",
        "echo 'the new gateway did not become ready' >&2",
        "exit 1",
        "",
    ];
    // `format` reads `%`: the script has no literal percent sign besides its five placeholders.
    let body = hcl_string(&lines.join("\n"));
    let peer = format!(
        "cidrhost(azurerm_subnet.{network}_sandbox_gateway.address_prefixes[0], {peer_host})"
    );
    expr::raw(format!(
        "format(\"{body}\", {peer}, azurerm_user_assigned_identity.{label}.client_id, var.azure_subscription_id, data.azurerm_client_config.{vault}_current.tenant_id, jsonencode({{service = \"key-vault\", vaultName = azurerm_key_vault.{vault}.name}}))"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: &str = "registry.example/gw@sha256:abc";

    /// The install script of the VM at `index`, as HCL.
    fn script(index: usize) -> String {
        let body = hcl::Body::builder()
            .add_attribute((
                "script",
                install_script(
                    IMAGE,
                    "secrets",
                    "net_sandbox_gateway",
                    "net",
                    GATEWAY_HOSTS[1 - index],
                ),
            ))
            .build();
        hcl::to_string(&body).unwrap()
    }

    /// First-boot custom data, as HCL.
    fn custom_data() -> String {
        let body = hcl::Body::builder()
            .add_attribute(("custom_data", cloud_config()))
            .build();
        hcl::to_string(&body).unwrap()
    }

    fn code() -> &'static str {
        let src = include_str!("sandbox_gateway_vm.rs");
        &src[..src.find("#[cfg(test)]").unwrap()]
    }

    #[test]
    fn the_container_is_unprivileged_and_pinned() {
        let text = script(0);
        for needed in [
            "--cap-drop ALL",
            // The gateway sizes its policy store from a finite cgroup memory.max and exits without one.
            "--memory 2g",
            "no-new-privileges",
            "--read-only",
            "--user 1000:1000",
            IMAGE,
        ] {
            assert!(text.contains(needed), "{needed} missing:\n{text}");
        }
        assert!(!text.contains("--network host") && !text.contains("--privileged"));
    }

    #[test]
    fn the_gateway_unit_logs_warnings_by_default() {
        let text = script(0);
        // The gateways used to log at error level only, which hid the refusals operators need.
        let env = text.find("/etc/alien-egress-gateway.env").unwrap();
        let unit = text.find("alien-egress-gateway.service").unwrap();
        let line = text.find("RUST_LOG=warn").expect(&text);
        assert!(
            env < line && line < unit,
            "RUST_LOG=warn is in the env file:\n{text}"
        );
        assert!(!text.contains("RUST_LOG=error") && !text.contains("RUST_LOG=info"));
    }

    #[test]
    fn docker_is_installed_by_a_retried_command_not_a_single_package_attempt() {
        let text = custom_data();
        // cloud-init's `packages` runs once; a VM that boots before its subnet has outbound
        // access (the NAT association) would never get Docker and the gateway would never start.
        assert!(!text.contains("packages:"), "{text}");
        assert!(text.contains("apt-get install -y docker.io"), "{text}");
        assert!(
            text.contains("seq 1 30") && text.contains("sleep 10"),
            "{text}"
        );
    }

    /// An Azure VM replaces itself when custom_data changes, and a setup re-run changes it the
    /// moment the image is inside it: both gateways would be replaced together and every sandbox
    /// would lose its egress. The image has to reach the VM another way.
    #[test]
    fn the_image_is_not_in_custom_data_so_an_image_change_never_replaces_a_vm() {
        let text = custom_data();
        assert!(
            !text.contains(IMAGE) && !text.contains("docker run") && !text.contains("ExecStart"),
            "{text}"
        );
        assert!(
            !text.contains("AZURE_CLIENT_ID"),
            "no deployment values in custom_data: {text}"
        );
        let code = code();
        assert!(
            code.contains("ignore_changes"),
            "installs written with the image inside keep their VMs"
        );
        assert!(code.contains("[custom_data]"));
        // The same custom data whatever the image is.
        assert_eq!(custom_data(), custom_data());
    }

    #[test]
    fn the_script_pulls_before_it_touches_the_running_gateway() {
        let text = script(0);
        let pull = text.find("docker pull").expect(&text);
        let write = text
            .find("cat > /etc/alien-egress-gateway.env")
            .expect(&text);
        let restart = text.find("systemctl restart").expect(&text);
        assert!(pull < write && write < restart, "{text}");
        assert!(
            text.contains(&format!("docker pull {IMAGE} || exit 1")),
            "{text}"
        );
    }

    #[test]
    fn a_serving_gateway_is_only_restarted_next_to_a_serving_peer_and_is_awaited() {
        let text = script(0);
        let check = text.find("the peer gateway is not serving").expect(&text);
        let restart = text.find("systemctl restart").expect(&text);
        assert!(
            check < restart,
            "the peer is checked before the restart:\n{text}"
        );
        // Ready means /health answers 200: the gateway reports it only once caught up from its peer.
        let wait = text[restart..].find("seq 1 60").expect(&text);
        assert!(
            text[restart + wait..].contains("did not become ready"),
            "{text}"
        );
        assert!(text.contains("3129/health"), "{text}");
        // A VM that was not serving (a fresh one: no configuration is published yet) is not awaited.
        assert!(text.contains(r#"[ \"$serving\" = yes ] || exit 0"#), "{text}");
    }

    #[test]
    fn each_script_checks_the_other_gateway_as_its_peer() {
        let src = code();
        assert!(src.contains("GATEWAY_HOSTS[1 - index]"));
        assert!(
            script(0)
                .contains("cidrhost(azurerm_subnet.net_sandbox_gateway.address_prefixes[0], 5)"),
            "{}",
            script(0)
        );
        assert!(
            script(1)
                .contains("cidrhost(azurerm_subnet.net_sandbox_gateway.address_prefixes[0], 4)"),
            "{}",
            script(1)
        );
    }

    #[test]
    fn the_script_survives_terraform_format_and_hcl_templates() {
        let text = script(0);
        // `format` reads `%` and HCL reads `${` and `%{`: the script has neither.
        let raw = hcl_string("a ${b} %{c} d\\e \"f\"");
        assert_eq!(raw, "a $${b} %%{c} d\\\\e \\\"f\\\"");
        assert!(!text.contains("%{"), "{text}");
        assert_eq!(
            text.matches("%s").count(),
            5,
            "five Terraform values:\n{text}"
        );
    }

    #[test]
    fn the_vms_wait_for_the_subnet_outbound_path() {
        let src = include_str!("sandbox_gateway_vm.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        assert!(
            code.contains("azurerm_subnet_nat_gateway_association"),
            "the VM depends on the NAT association"
        );
        assert!(code.contains("azurerm_subnet_network_security_group_association"));
        assert!(code.contains("\"depends_on\""));
    }

    #[test]
    fn the_terraform_values_fill_the_placeholders() {
        let text = script(0);
        assert_eq!(text.matches("%s").count(), 5, "{text}");
        // jsonencode keeps the vault binding valid JSON whatever the vault name holds.
        assert!(text.contains("jsonencode"));
    }

    #[test]
    fn the_vm_size_is_a_documented_variable_with_the_current_default() {
        let text = hcl::to_string(
            &hcl::Body::builder()
                .add_block(vm_size_variable("net_sandbox_gateway"))
                .build(),
        )
        .unwrap();
        assert!(
            text.contains("variable \"net_sandbox_gateway_vm_size\""),
            "{text}"
        );
        assert!(
            text.contains("default     = \"Standard_D2s_v5\"")
                || text.contains("default = \"Standard_D2s_v5\""),
            "{text}"
        );
        assert!(
            text.contains("at least 2 vCPUs") && text.contains("quota"),
            "{text}"
        );
        assert!(text.contains("validation"), "{text}");
        let src = include_str!("sandbox_gateway_vm.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        assert!(code.contains("var.{label}_vm_size"));
        assert_eq!(code.matches("\"size\"").count(), 1);
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
