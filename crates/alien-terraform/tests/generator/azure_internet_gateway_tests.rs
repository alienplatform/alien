//! The Azure `internet` sandbox's gateway VMs, checked on the rendered module (parsed HCL, not the
//! emitter's source): the order a setup re-run updates them in, and the VM image pin.

use super::helpers::{prepare_for_target_unchecked, render};
use alien_core::{
    ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy, Stack,
    StackSettings,
};
use alien_terraform::TerraformTarget;
use hcl::Body;

async fn gateway_body() -> Body {
    let sandbox = Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "registry.example/agents:1".to_string(),
            bundled_from: None,
        })
        .egress(SandboxEgress::Internet)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
            ..Default::default()
        })
        .build();
    let stack = Stack::new("azint".to_string())
        .add(sandbox, ResourceLifecycle::Frozen)
        .build();
    let prepared = prepare_for_target_unchecked(stack, TerraformTarget::Azure).await;
    let module = render(&prepared, TerraformTarget::Azure, StackSettings::default());
    let mut merged = Body::builder();
    for (path, contents) in module.iter() {
        if path.ends_with(".tf") && contents.contains("azurerm_virtual_machine_run_command") {
            let body: Body = hcl::from_str(contents).expect("the rendered file parses as HCL");
            for structure in body.into_iter() {
                merged = merged.add_structure(structure);
            }
        }
    }
    merged.build()
}

fn resource<'a>(body: &'a Body, kind: &str, label_suffix: &str) -> &'a hcl::Block {
    body.blocks()
        .find(|b| {
            b.identifier() == "resource"
                && b.labels().first().map(|l| l.as_str()) == Some(kind)
                && b.labels()
                    .get(1)
                    .is_some_and(|l| l.as_str().ends_with(label_suffix))
        })
        .unwrap_or_else(|| panic!("{kind} *{label_suffix} is rendered"))
}

fn depends_on(block: &hcl::Block) -> String {
    block
        .body()
        .attributes()
        .find(|a| a.key() == "depends_on")
        .map(|a| a.expr().to_string())
        .unwrap_or_default()
}

#[tokio::test]
async fn the_second_gateways_run_command_waits_for_the_first_in_the_rendered_module() {
    let body = gateway_body().await;
    let first = resource(
        &body,
        "azurerm_virtual_machine_run_command",
        "_sandbox_gateway_0",
    );
    let second = resource(
        &body,
        "azurerm_virtual_machine_run_command",
        "_sandbox_gateway_1",
    );
    assert!(
        depends_on(second)
            .contains("azurerm_virtual_machine_run_command.default_network_sandbox_gateway_0"),
        "VM 1's command waits for VM 0's: {}",
        depends_on(second)
    );
    assert!(
        !depends_on(first)
            .contains("azurerm_virtual_machine_run_command.default_network_sandbox_gateway_1"),
        "VM 0's command waits for nothing of VM 1's: {}",
        depends_on(first)
    );
    // Repairs of an unhealthy VM run first, one VM at a time, before either update.
    let repair0 = resource(
        &body,
        "azurerm_virtual_machine_run_command",
        "_sandbox_gateway_repair_0",
    );
    let repair1 = resource(
        &body,
        "azurerm_virtual_machine_run_command",
        "_sandbox_gateway_repair_1",
    );
    assert!(
        depends_on(repair1).contains(
            "azurerm_virtual_machine_run_command.default_network_sandbox_gateway_repair_0"
        ),
        "{}",
        depends_on(repair1)
    );
    assert!(!depends_on(repair0).contains("azurerm_virtual_machine_run_command"));
    for update in [first, second] {
        for repair in ["repair_0", "repair_1"] {
            assert!(
                depends_on(update).contains(&format!(
                    "azurerm_virtual_machine_run_command.default_network_sandbox_gateway_{repair}"
                )),
                "an update waits for the repairs: {}",
                depends_on(update)
            );
        }
    }
    // The image is in the run command, never in custom_data (a custom_data change replaces a VM).
    for vm in ["_sandbox_gateway_0", "_sandbox_gateway_1"] {
        let machine = resource(&body, "azurerm_linux_virtual_machine", vm);
        let attributes: Vec<_> = machine.body().attributes().collect();
        let custom_data = attributes
            .iter()
            .find(|a| a.key() == "custom_data")
            .expect("custom_data");
        assert!(
            !custom_data.expr().to_string().contains("registry.example"),
            "{}",
            custom_data.expr()
        );
        let lifecycle = machine
            .body()
            .blocks()
            .find(|b| b.identifier() == "lifecycle")
            .expect("lifecycle");
        assert!(
            lifecycle.body().attributes().any(
                |a| a.key() == "ignore_changes" && a.expr().to_string().contains("custom_data")
            ),
            "installs written with the image in custom_data keep their VMs"
        );
    }
}

#[tokio::test]
async fn the_gateway_vms_use_a_pinned_ubuntu_image() {
    let body = gateway_body().await;
    for vm in ["_sandbox_gateway_0", "_sandbox_gateway_1"] {
        let machine = resource(&body, "azurerm_linux_virtual_machine", vm);
        let image = machine
            .body()
            .blocks()
            .find(|b| b.identifier() == "source_image_reference")
            .expect("source_image_reference");
        let version = image
            .body()
            .attributes()
            .find(|a| a.key() == "version")
            .expect("version")
            .expr()
            .to_string();
        assert!(!version.contains("latest"), "{version}");
        assert!(version.contains("24.04."), "{version}");
    }
}
