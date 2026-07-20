use crate::context::ComputeTestContext;
use alien_gcp_clients::compute::{ComputeApi, Disk};
use test_context::test_context;

// =============================================================================================
// Comprehensive E2E Test - Persistent Disk
// =============================================================================================

#[test_context(ComputeTestContext)]
#[tokio::test]
async fn test_comprehensive_disk_lifecycle(ctx: &mut ComputeTestContext) {
    println!("🚀 Starting comprehensive Persistent Disk lifecycle test");

    let disk_name = ctx.generate_unique_name("disk");

    // =========================================================================
    // Step 1: Create Disk
    // =========================================================================
    println!("\n📦 Step 1: Creating disk: {}", disk_name);

    let disk = Disk::builder()
        .name(disk_name.clone())
        .description("Alien test persistent disk".to_string())
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
        .expect("Failed to create disk");

    ctx.track_disk(&ctx.zone, &disk_name);
    assert!(
        create_disk_op.name.is_some(),
        "Create disk operation should have a name"
    );
    println!("✅ Disk creation initiated");

    ctx.wait_for_zone_operation(&ctx.zone, create_disk_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Disk creation timed out");

    // Verify disk was created
    let fetched_disk = ctx
        .client
        .get_disk(ctx.zone.clone(), disk_name.clone())
        .await
        .expect("Failed to get disk");

    assert_eq!(fetched_disk.name.as_ref().unwrap(), &disk_name);
    assert_eq!(fetched_disk.size_gb, Some("10".to_string()));
    println!("✅ Disk verified: {}", disk_name);

    // =========================================================================
    // Step 2: Delete Disk
    // =========================================================================
    println!("\n🧹 Step 2: Deleting disk");

    let delete_disk_op = ctx
        .client
        .delete_disk(ctx.zone.clone(), disk_name.clone())
        .await
        .expect("Failed to delete disk");

    ctx.wait_for_zone_operation(&ctx.zone, delete_disk_op.name.as_ref().unwrap(), 120)
        .await
        .expect("Disk deletion timed out");
    ctx.untrack_disk(&ctx.zone, &disk_name);
    println!("✅ Disk deleted");

    // Verify disk was deleted (should return 404)
    let result = ctx
        .client
        .get_disk(ctx.zone.clone(), disk_name.clone())
        .await;
    assert!(result.is_err(), "Disk should be deleted");

    println!("\n🎉 Comprehensive Persistent Disk lifecycle test completed successfully!");
    println!("   - Disk created and deleted: ✅");
}
