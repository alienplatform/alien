# SyncListResponsePreparedStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncListResponsePreparedStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackPermissionSetPlatforms = {};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                    | [models.SyncListResponsePreparedStackPermissionSetAw](../models/synclistresponsepreparedstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                                       | AWS permission configurations                                                                                            |
| `azure`                                                                                                                  | [models.SyncListResponsePreparedStackPermissionSetAzure](../models/synclistresponsepreparedstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                                       | Azure permission configurations                                                                                          |
| `gcp`                                                                                                                    | [models.SyncListResponsePreparedStackPermissionSetGcp](../models/synclistresponsepreparedstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                                       | GCP permission configurations                                                                                            |