# KeyHeartbeatDataGcpCloudKms

## Example Usage

```typescript
import { KeyHeartbeatDataGcpCloudKms } from "@alienplatform/manager-api/models";

let value: KeyHeartbeatDataGcpCloudKms = {
  data: {
    cryptoKeyName: "<value>",
    purpose: "<value>",
    status: {
      health: "unknown",
      lifecycle: "deleted",
    },
  },
  provider: "gcp-cloud-kms",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `data`                                                                         | [models.GcpCloudKmsKeyHeartbeatData](../models/gcpcloudkmskeyheartbeatdata.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `provider`                                                                     | *"gcp-cloud-kms"*                                                              | :heavy_check_mark:                                                             | N/A                                                                            |