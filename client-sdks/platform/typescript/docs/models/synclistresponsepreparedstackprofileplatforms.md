# SyncListResponsePreparedStackProfilePlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncListResponsePreparedStackProfilePlatforms } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackProfilePlatforms = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                        | [models.SyncListResponsePreparedStackProfileAw](../models/synclistresponsepreparedstackprofileaw.md)[]       | :heavy_minus_sign:                                                                                           | AWS permission configurations                                                                                |
| `azure`                                                                                                      | [models.SyncListResponsePreparedStackProfileAzure](../models/synclistresponsepreparedstackprofileazure.md)[] | :heavy_minus_sign:                                                                                           | Azure permission configurations                                                                              |
| `gcp`                                                                                                        | [models.SyncListResponsePreparedStackProfileGcp](../models/synclistresponsepreparedstackprofilegcp.md)[]     | :heavy_minus_sign:                                                                                           | GCP permission configurations                                                                                |
