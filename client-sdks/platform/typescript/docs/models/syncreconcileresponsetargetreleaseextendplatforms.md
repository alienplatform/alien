# SyncReconcileResponseTargetReleaseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileResponseTargetReleaseExtendPlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseTargetReleaseExtendPlatforms = {};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                | [models.SyncReconcileResponseTargetReleaseExtendAw](../models/syncreconcileresponsetargetreleaseextendaw.md)[]       | :heavy_minus_sign:                                                                                                   | AWS permission configurations                                                                                        |
| `azure`                                                                                                              | [models.SyncReconcileResponseTargetReleaseExtendAzure](../models/syncreconcileresponsetargetreleaseextendazure.md)[] | :heavy_minus_sign:                                                                                                   | Azure permission configurations                                                                                      |
| `gcp`                                                                                                                | [models.SyncReconcileResponseTargetReleaseExtendGcp](../models/syncreconcileresponsetargetreleaseextendgcp.md)[]     | :heavy_minus_sign:                                                                                                   | GCP permission configurations                                                                                        |