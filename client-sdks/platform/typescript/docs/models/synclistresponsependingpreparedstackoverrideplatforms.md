# SyncListResponsePendingPreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                        | [models.SyncListResponsePendingPreparedStackOverrideAw](../models/synclistresponsependingpreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                           | AWS permission configurations                                                                                                |
| `azure`                                                                                                                      | [models.SyncListResponsePendingPreparedStackOverrideAzure](../models/synclistresponsependingpreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                           | Azure permission configurations                                                                                              |
| `gcp`                                                                                                                        | [models.SyncListResponsePendingPreparedStackOverrideGcp](../models/synclistresponsependingpreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                           | GCP permission configurations                                                                                                |
