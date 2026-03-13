# SyncReconcileResponseCurrentReleaseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseExtendPlatforms } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseExtendPlatforms = {};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                  | [models.SyncReconcileResponseCurrentReleaseExtendAw](../models/syncreconcileresponsecurrentreleaseextendaw.md)[]       | :heavy_minus_sign:                                                                                                     | AWS permission configurations                                                                                          |
| `azure`                                                                                                                | [models.SyncReconcileResponseCurrentReleaseExtendAzure](../models/syncreconcileresponsecurrentreleaseextendazure.md)[] | :heavy_minus_sign:                                                                                                     | Azure permission configurations                                                                                        |
| `gcp`                                                                                                                  | [models.SyncReconcileResponseCurrentReleaseExtendGcp](../models/syncreconcileresponsecurrentreleaseextendgcp.md)[]     | :heavy_minus_sign:                                                                                                     | GCP permission configurations                                                                                          |