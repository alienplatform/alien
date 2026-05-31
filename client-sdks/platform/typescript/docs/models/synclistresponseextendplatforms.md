# SyncListResponseExtendPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncListResponseExtendPlatforms } from "@alienplatform/platform-api/models";

let value: SyncListResponseExtendPlatforms = {};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `aws`                                                                            | [models.SyncListResponseExtendAw](../models/synclistresponseextendaw.md)[]       | :heavy_minus_sign:                                                               | AWS permission configurations                                                    |
| `azure`                                                                          | [models.SyncListResponseExtendAzure](../models/synclistresponseextendazure.md)[] | :heavy_minus_sign:                                                               | Azure permission configurations                                                  |
| `gcp`                                                                            | [models.SyncListResponseExtendGcp](../models/synclistresponseextendgcp.md)[]     | :heavy_minus_sign:                                                               | GCP permission configurations                                                    |