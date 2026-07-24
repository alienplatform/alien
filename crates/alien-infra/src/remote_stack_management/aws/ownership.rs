use super::*;

impl AwsRemoteStackManagementController {
    pub(super) fn setup_managed_resources(&self) -> bool {
        // AWS used the same role name for the historical direct-create and
        // setup-import paths, so old checkpoints do not contain enough durable
        // evidence to infer ownership safely. Preserve their original runtime
        // behavior; all new imports persist setup ownership explicitly.
        self.setup_managed.unwrap_or(false)
    }
}
