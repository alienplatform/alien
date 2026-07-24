# SyncListResponsePreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncListResponsePreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                          | [models.SyncListResponsePreparedStackOverrideAw](../models/synclistresponsepreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                             | AWS permission configurations                                                                                  |
| `azure`                                                                                                        | [models.SyncListResponsePreparedStackOverrideAzure](../models/synclistresponsepreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                             | Azure permission configurations                                                                                |
| `gcp`                                                                                                          | [models.SyncListResponsePreparedStackOverrideGcp](../models/synclistresponsepreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                             | GCP permission configurations                                                                                  |
