use crate::context::ComputeTestContext;
use alien_gcp_clients::compute::{
    AttachedDisk, AttachedDiskInitializeParams, AttachedDiskType, ComputeApi, Disk, DiskMode,
    FixedOrPercent, InstanceGroupManager, InstanceGroupManagerUpdatePolicy, InstanceProperties,
    InstanceTemplate, MinimalAction, NetworkInterface, ServiceAccount, UpdatePolicyType,
};
use test_context::test_context;

// =============================================================================================
// Comprehensive E2E Test - Instance Management
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_comprehensive_instance_management_lifecycle(ctx: &mut ComputeTestContext) {
    println!("🚀 Starting comprehensive Instance Management lifecycle test");

    let template_name = ctx.generate_unique_name("template");
    let igm_name = ctx.generate_unique_name("igm");

    // =========================================================================
    // Step 1: Create Instance Template
    // =========================================================================
    println!("\n📦 Step 1: Creating instance template: {}", template_name);

    let instance_template = InstanceTemplate::builder()
        .name(template_name.clone())
        .description("Alien test instance template".to_string())
        .properties(
            InstanceProperties::builder()
                .machine_type("e2-micro".to_string())
                .disks(vec![AttachedDisk::builder()
                    .r#type(AttachedDiskType::Persistent)
                    .boot(true)
                    .mode(DiskMode::ReadWrite)
                    .auto_delete(true)
                    .initialize_params(
                        AttachedDiskInitializeParams::builder()
                            .source_image(
                                "projects/debian-cloud/global/images/family/debian-11".to_string(),
                            )
                            .disk_size_gb("10".to_string())
                            .build(),
                    )
                    .build()])
                .network_interfaces(vec![NetworkInterface::builder()
                    .network("global/networks/default".to_string())
                    .build()])
                .service_accounts(vec![ServiceAccount::builder()
                    .email("default".to_string())
                    .scopes(vec![
                        "https://www.googleapis.com/auth/cloud-platform".to_string()
                    ])
                    .build()])
                .build(),
        )
        .build();

    let create_template_op = ctx
        .client
        .insert_instance_template(instance_template)
        .await
        .expect("Failed to create instance template");

    ctx.track_instance_template(&template_name);
    assert!(
        create_template_op.name.is_some(),
        "Create template operation should have a name"
    );
    println!("✅ Instance template creation initiated");

    ctx.wait_for_global_operation(create_template_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Instance template creation timed out");

    // Verify template was created
    let fetched_template = ctx
        .client
        .get_instance_template(template_name.clone())
        .await
        .expect("Failed to get instance template");

    assert_eq!(fetched_template.name.as_ref().unwrap(), &template_name);
    println!("✅ Instance template verified: {}", template_name);

    // =========================================================================
    // Step 2: Create Instance Group Manager
    // =========================================================================
    println!("\n📦 Step 2: Creating instance group manager: {}", igm_name);

    let template_url = format!(
        "projects/{}/global/instanceTemplates/{}",
        ctx.project_id, template_name
    );

    let igm = InstanceGroupManager::builder()
        .name(igm_name.clone())
        .description("Alien test instance group manager".to_string())
        .instance_template(template_url)
        .base_instance_name(format!("alien-test-{}", &igm_name[..8.min(igm_name.len())]))
        .target_size(0) // Start with 0 instances
        .update_policy(
            InstanceGroupManagerUpdatePolicy::builder()
                .r#type(UpdatePolicyType::Proactive)
                .build(),
        )
        .build();

    let create_igm_op = ctx
        .client
        .insert_instance_group_manager(ctx.zone.clone(), igm)
        .await
        .expect("Failed to create instance group manager");

    ctx.track_instance_group_manager(&ctx.zone, &igm_name);
    assert!(
        create_igm_op.name.is_some(),
        "Create IGM operation should have a name"
    );
    println!("✅ Instance group manager creation initiated");

    ctx.wait_for_zone_operation(&ctx.zone, create_igm_op.name.as_ref().unwrap(), 180)
        .await
        .expect("Instance group manager creation timed out");

    // Verify IGM was created
    let fetched_igm = ctx
        .client
        .get_instance_group_manager(ctx.zone.clone(), igm_name.clone())
        .await
        .expect("Failed to get instance group manager");

    assert_eq!(fetched_igm.name.as_ref().unwrap(), &igm_name);
    assert_eq!(fetched_igm.target_size, Some(0));
    println!("✅ Instance group manager verified: {}", igm_name);

    // =========================================================================
    // Step 3: Resize Instance Group Manager
    // =========================================================================
    println!("\n📦 Step 3: Resizing instance group manager to 1 instance");

    let resize_op = ctx
        .client
        .resize_instance_group_manager(ctx.zone.clone(), igm_name.clone(), 1)
        .await
        .expect("Failed to resize IGM");

    assert!(
        resize_op.name.is_some(),
        "Resize operation should have a name"
    );
    println!("✅ Resize operation initiated");

    ctx.wait_for_zone_operation(&ctx.zone, resize_op.name.as_ref().unwrap(), 300)
        .await
        .expect("Resize operation timed out");

    // Wait a bit for instances to be created
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    // =========================================================================
    // Step 3.5: Create a second instance template for patch test
    // =========================================================================
    println!("\n📦 Step 3.5: Creating second instance template for patch test");

    let template_v2_name = ctx.generate_unique_name("template-v2");

    let instance_template_v2 = InstanceTemplate::builder()
        .name(template_v2_name.clone())
        .description(
            "Alien test instance template v2 (for patch_instance_group_manager)".to_string(),
        )
        .properties(
            InstanceProperties::builder()
                .machine_type("e2-micro".to_string())
                .disks(vec![AttachedDisk::builder()
                    .r#type(AttachedDiskType::Persistent)
                    .boot(true)
                    .mode(DiskMode::ReadWrite)
                    .auto_delete(true)
                    .initialize_params(
                        AttachedDiskInitializeParams::builder()
                            .source_image(
                                "projects/debian-cloud/global/images/family/debian-11".to_string(),
                            )
                            .disk_size_gb("10".to_string())
                            .build(),
                    )
                    .build()])
                .network_interfaces(vec![NetworkInterface::builder()
                    .network("global/networks/default".to_string())
                    .build()])
                .service_accounts(vec![ServiceAccount::builder()
                    .email("default".to_string())
                    .scopes(vec![
                        "https://www.googleapis.com/auth/cloud-platform".to_string()
                    ])
                    .build()])
                .build(),
        )
        .build();

    let create_template_v2_op = ctx
        .client
        .insert_instance_template(instance_template_v2)
        .await
        .expect("Failed to create second instance template");

    ctx.track_instance_template(&template_v2_name);
    ctx.wait_for_global_operation(create_template_v2_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Second instance template creation timed out");
    println!("✅ Second instance template created: {}", template_v2_name);

    // =========================================================================
    // Step 3.6: Patch the IGM to use the new template with PROACTIVE update policy
    // =========================================================================
    println!("\n📦 Step 3.6: Patching IGM to use new template with PROACTIVE rolling update");

    let template_v2_url = format!(
        "https://compute.googleapis.com/compute/v1/projects/{}/global/instanceTemplates/{}",
        ctx.project_id, template_v2_name
    );

    let igm_patch = InstanceGroupManager::builder()
        .instance_template(template_v2_url.clone())
        .update_policy(InstanceGroupManagerUpdatePolicy {
            r#type: Some(UpdatePolicyType::Proactive),
            minimal_action: Some(MinimalAction::Replace),
            most_disruptive_allowed_action: None,
            // maxSurge: 1 — create 1 extra VM before terminating old (works with target_size=1)
            max_surge: Some(FixedOrPercent {
                fixed: Some(1),
                percent: None,
                calculated: None,
            }),
            // maxUnavailable: 0 — never reduce capacity below target_size
            max_unavailable: Some(FixedOrPercent {
                fixed: Some(0),
                percent: None,
                calculated: None,
            }),
            replacement_method: None,
        })
        .build();

    let patch_op = ctx
        .client
        .patch_instance_group_manager(ctx.zone.clone(), igm_name.clone(), igm_patch)
        .await
        .expect("Failed to patch instance group manager");

    assert!(
        patch_op.name.is_some(),
        "Patch operation should have a name"
    );
    println!("✅ Patch operation initiated: {:?}", patch_op.name);

    ctx.wait_for_zone_operation(&ctx.zone, patch_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Patch operation timed out");

    // =========================================================================
    // Step 3.7: Verify the IGM now references the new template
    // =========================================================================
    println!("\n📦 Step 3.7: Verifying IGM references new template after patch");

    let patched_igm = ctx
        .client
        .get_instance_group_manager(ctx.zone.clone(), igm_name.clone())
        .await
        .expect("Failed to get patched IGM");

    let current_template = patched_igm.instance_template.as_deref().unwrap_or("");
    assert!(
        current_template.contains(&template_v2_name),
        "IGM should now reference the new template '{}', but has '{}'",
        template_v2_name,
        current_template
    );
    println!(
        "✅ IGM correctly references new template: {}",
        template_v2_name
    );

    let stable_managed_instance = ctx
        .wait_for_stable_managed_instance(&ctx.zone, &igm_name, &template_v2_name, 300)
        .await
        .expect("Managed instance group never converged on the patched template");

    // =========================================================================
    // Step 4: List Managed Instances
    // =========================================================================
    println!("\n📦 Step 4: Listing managed instances");

    let managed_instances = ctx
        .client
        .list_managed_instances(ctx.zone.clone(), igm_name.clone())
        .await
        .expect("Failed to list managed instances");

    println!(
        "  Found {} managed instances",
        managed_instances.managed_instances.len()
    );
    for mi in &managed_instances.managed_instances {
        if let Some(instance_url) = &mi.instance {
            let instance_name = instance_url.split('/').last().unwrap_or("unknown");
            println!(
                "    - Instance: {}, Status: {:?}",
                instance_name, mi.instance_status
            );
        }
    }
    println!("✅ Managed instances listed");

    // =========================================================================
    // Step 4.1: Get serial port output from the managed instance
    // =========================================================================
    println!("\n📦 Step 4.1: Reading serial port output from first instance");

    let stable_instance_url = stable_managed_instance
        .instance
        .as_ref()
        .expect("Stable managed instance should have an instance URL");
    let stable_instance_name = stable_instance_url
        .split('/')
        .last()
        .unwrap_or("unknown")
        .to_string();

    // Retry because the instance may not be ready immediately after creation or replacement.
    let mut serial_output = None;
    for attempt in 1..=12 {
        match ctx
            .client
            .get_serial_port_output(ctx.zone.clone(), stable_instance_name.clone())
            .await
        {
            Ok(output) => {
                serial_output = Some(output);
                break;
            }
            Err(e) => {
                let msg = format!("{:?}", e);
                if msg.contains("resourceNotReady") || msg.contains("not ready") {
                    println!(
                        "  Instance not ready for serial port (attempt {}/12), waiting 10s...",
                        attempt
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                } else {
                    panic!("Failed to get serial port output: {:?}", e);
                }
            }
        }
    }

    let serial_output =
        serial_output.expect("Instance never became ready for serial port output after 120s");
    println!(
        "  Serial port output length: {} bytes",
        serial_output.contents.as_deref().unwrap_or("").len()
    );
    assert!(
        serial_output.contents.is_some(),
        "Serial port output should have a contents field"
    );
    println!(
        "✅ Serial port output retrieved successfully from {}",
        stable_instance_name
    );

    // =========================================================================
    // Step 4.5: Attach and detach a persistent disk to a managed instance
    // =========================================================================
    println!("\n📦 Step 4.5: Attaching and detaching a disk to a managed instance");

    let instance_name = stable_instance_name.clone();

    let disk_name = ctx.generate_unique_name("attach-disk");
    let device_name = format!("dev-{}", &disk_name[..8.min(disk_name.len())]);
    let disk = Disk::builder()
        .name(disk_name.clone())
        .description("Alien test attached disk".to_string())
        .size_gb("10".to_string())
        .r#type(format!(
            "projects/{}/zones/{}/diskTypes/pd-standard",
            ctx.project_id, ctx.zone
        ))
        .build();

    let create_disk_op = ctx
        .client
        .insert_disk(ctx.zone.clone(), disk)
        .await
        .expect("Failed to create disk for attachment");
    ctx.track_disk(&ctx.zone, &disk_name);

    ctx.wait_for_zone_operation(&ctx.zone, create_disk_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Disk creation timed out");

    let attached_disk = AttachedDisk::builder()
        .r#type(AttachedDiskType::Persistent)
        .mode(DiskMode::ReadWrite)
        .source(format!(
            "projects/{}/zones/{}/disks/{}",
            ctx.project_id, ctx.zone, disk_name
        ))
        .device_name(device_name.clone())
        .auto_delete(false)
        .build();

    let attach_op = ctx
        .client
        .attach_disk(ctx.zone.clone(), instance_name.clone(), attached_disk)
        .await
        .expect("Failed to attach disk");

    ctx.wait_for_zone_operation(&ctx.zone, attach_op.name.as_ref().unwrap(), 300)
        .await
        .expect("Disk attach operation failed or timed out");
    println!("✅ Disk attached to instance {}", instance_name);

    let detach_op = ctx
        .client
        .detach_disk(ctx.zone.clone(), instance_name.clone(), device_name.clone())
        .await
        .expect("Failed to detach disk");

    ctx.wait_for_zone_operation(&ctx.zone, detach_op.name.as_ref().unwrap(), 300)
        .await
        .expect("Disk detach operation failed or timed out");
    println!("✅ Disk detached from instance {}", instance_name);

    let delete_disk_op = ctx
        .client
        .delete_disk(ctx.zone.clone(), disk_name.clone())
        .await
        .expect("Failed to delete attached disk");
    ctx.wait_for_zone_operation(&ctx.zone, delete_disk_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Disk deletion failed or timed out");
    ctx.untrack_disk(&ctx.zone, &disk_name);
    println!("✅ Attached disk deleted");

    // =========================================================================
    // Step 5: Resize back to 0 before cleanup
    // =========================================================================
    println!("\n📦 Step 5: Resizing instance group manager back to 0");

    let resize_down_op = ctx
        .client
        .resize_instance_group_manager(ctx.zone.clone(), igm_name.clone(), 0)
        .await
        .expect("Failed to resize IGM down");

    ctx.wait_for_zone_operation(&ctx.zone, resize_down_op.name.as_ref().unwrap(), 300)
        .await
        .expect("Resize down operation timed out");
    println!("✅ Instance group manager resized to 0");

    // Wait for instances to be deleted
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    // =========================================================================
    // Step 6: Cleanup
    // =========================================================================
    println!("\n🧹 Step 6: Cleaning up instance management resources");

    // Delete IGM
    let delete_igm_op = ctx
        .client
        .delete_instance_group_manager(ctx.zone.clone(), igm_name.clone())
        .await
        .expect("Failed to delete IGM");

    ctx.wait_for_zone_operation(&ctx.zone, delete_igm_op.name.as_ref().unwrap(), 300)
        .await
        .expect("IGM deletion timed out");
    ctx.untrack_instance_group_manager(&ctx.zone, &igm_name);
    println!("  ✅ Instance group manager deleted");

    // Delete instance template
    let delete_template_op = ctx
        .client
        .delete_instance_template(template_name.clone())
        .await
        .expect("Failed to delete instance template");

    ctx.wait_for_global_operation(delete_template_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Instance template deletion timed out");
    ctx.untrack_instance_template(&template_name);
    println!("  ✅ Instance template deleted");

    println!("\n🎉 Comprehensive Instance Management lifecycle test completed successfully!");
    println!("   - Instance template created and deleted: ✅");
    println!("   - Instance group manager created, resized, and deleted: ✅");
    println!("   - Managed instances listed: ✅");
}
