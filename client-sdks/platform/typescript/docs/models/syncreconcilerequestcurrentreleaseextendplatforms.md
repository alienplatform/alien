# SyncReconcileRequestCurrentReleaseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileRequestCurrentReleaseExtendPlatforms } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestCurrentReleaseExtendPlatforms = {};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                | [models.SyncReconcileRequestCurrentReleaseExtendAw](../models/syncreconcilerequestcurrentreleaseextendaw.md)[]       | :heavy_minus_sign:                                                                                                   | AWS permission configurations                                                                                        |
| `azure`                                                                                                              | [models.SyncReconcileRequestCurrentReleaseExtendAzure](../models/syncreconcilerequestcurrentreleaseextendazure.md)[] | :heavy_minus_sign:                                                                                                   | Azure permission configurations                                                                                      |
| `gcp`                                                                                                                | [models.SyncReconcileRequestCurrentReleaseExtendGcp](../models/syncreconcilerequestcurrentreleaseextendgcp.md)[]     | :heavy_minus_sign:                                                                                                   | GCP permission configurations                                                                                        |