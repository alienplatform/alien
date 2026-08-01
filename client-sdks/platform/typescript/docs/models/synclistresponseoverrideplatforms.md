# SyncListResponseOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncListResponseOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncListResponseOverridePlatforms = {};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `aws`                                                                                | [models.SyncListResponseOverrideAw](../models/synclistresponseoverrideaw.md)[]       | :heavy_minus_sign:                                                                   | AWS permission configurations                                                        |
| `azure`                                                                              | [models.SyncListResponseOverrideAzure](../models/synclistresponseoverrideazure.md)[] | :heavy_minus_sign:                                                                   | Azure permission configurations                                                      |
| `gcp`                                                                                | [models.SyncListResponseOverrideGcp](../models/synclistresponseoverridegcp.md)[]     | :heavy_minus_sign:                                                                   | GCP permission configurations                                                        |