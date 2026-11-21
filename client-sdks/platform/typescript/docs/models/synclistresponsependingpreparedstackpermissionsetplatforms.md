# SyncListResponsePendingPreparedStackPermissionSetPlatforms

Platform-specific permission configurations

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackPermissionSetPlatforms } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackPermissionSetPlatforms = {};
```

## Fields

| Field                                                                                                                                  | Type                                                                                                                                   | Required                                                                                                                               | Description                                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                                  | [models.SyncListResponsePendingPreparedStackPermissionSetAw](../models/synclistresponsependingpreparedstackpermissionsetaw.md)[]       | :heavy_minus_sign:                                                                                                                     | AWS permission configurations                                                                                                          |
| `azure`                                                                                                                                | [models.SyncListResponsePendingPreparedStackPermissionSetAzure](../models/synclistresponsependingpreparedstackpermissionsetazure.md)[] | :heavy_minus_sign:                                                                                                                     | Azure permission configurations                                                                                                        |
| `gcp`                                                                                                                                  | [models.SyncListResponsePendingPreparedStackPermissionSetGcp](../models/synclistresponsependingpreparedstackpermissionsetgcp.md)[]     | :heavy_minus_sign:                                                                                                                     | GCP permission configurations                                                                                                          |