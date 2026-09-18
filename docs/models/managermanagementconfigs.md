# ManagerManagementConfigs

Per-platform management configurations for cross-account access (self-reported via heartbeat)

## Example Usage

```typescript
import { ManagerManagementConfigs } from "@alienplatform/platform-api/models";

let value: ManagerManagementConfigs = {};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `aws`                                                                                        | [models.ManagerManagementConfigsAws](../models/managermanagementconfigsaws.md)               | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `gcp`                                                                                        | [models.ManagerManagementConfigsGcp](../models/managermanagementconfigsgcp.md)               | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `azure`                                                                                      | [models.ManagerManagementConfigsAzure](../models/managermanagementconfigsazure.md)           | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `kubernetes`                                                                                 | [models.ManagerManagementConfigsKubernetes](../models/managermanagementconfigskubernetes.md) | :heavy_minus_sign:                                                                           | N/A                                                                                          |