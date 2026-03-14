# PlatformPermissions

Platform-specific permission configurations

## Example Usage

```typescript
import { PlatformPermissions } from "@alienplatform/manager-api/models";

let value: PlatformPermissions = {};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `aws`                                                                    | [models.AwsPlatformPermission](../models/awsplatformpermission.md)[]     | :heavy_minus_sign:                                                       | AWS permission configurations                                            |
| `azure`                                                                  | [models.AzurePlatformPermission](../models/azureplatformpermission.md)[] | :heavy_minus_sign:                                                       | Azure permission configurations                                          |
| `gcp`                                                                    | [models.GcpPlatformPermission](../models/gcpplatformpermission.md)[]     | :heavy_minus_sign:                                                       | GCP permission configurations                                            |