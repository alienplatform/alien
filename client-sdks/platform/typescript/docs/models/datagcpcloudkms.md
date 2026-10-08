# DataGcpCloudKms

## Example Usage

```typescript
import { DataGcpCloudKms } from "@alienplatform/platform-api/models";

let value: DataGcpCloudKms = {
  data: {
    cryptoKeyName: "<value>",
    purpose: "<value>",
    status: {
      health: "degraded",
      lifecycle: "running",
    },
  },
  provider: "gcp-cloud-kms",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `data`                                                                     | [models.SyncReconcileRequestData7](../models/syncreconcilerequestdata7.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `provider`                                                                 | *"gcp-cloud-kms"*                                                          | :heavy_check_mark:                                                         | N/A                                                                        |