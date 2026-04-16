# SyncAcquireResponsePreparedStackOverridePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackOverridePlatforms } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePreparedStackOverridePlatforms = {};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                | [models.SyncAcquireResponsePreparedStackOverrideAw](../models/syncacquireresponsepreparedstackoverrideaw.md)[]       | :heavy_minus_sign:                                                                                                   | AWS permission configurations                                                                                        |
| `azure`                                                                                                              | [models.SyncAcquireResponsePreparedStackOverrideAzure](../models/syncacquireresponsepreparedstackoverrideazure.md)[] | :heavy_minus_sign:                                                                                                   | Azure permission configurations                                                                                      |
| `gcp`                                                                                                                | [models.SyncAcquireResponsePreparedStackOverrideGcp](../models/syncacquireresponsepreparedstackoverridegcp.md)[]     | :heavy_minus_sign:                                                                                                   | GCP permission configurations                                                                                        |