# ResourceHeartbeatDataKey

## Example Usage

```typescript
import { ResourceHeartbeatDataKey } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataKey = {
  data: {
    data: {
      enabled: false,
      keyArn: "<value>",
      keySpec: "<value>",
      keyState: "<value>",
      keyUsage: "<value>",
      status: {
        health: "unknown",
        lifecycle: "deleted",
      },
    },
    provider: "aws-kms",
  },
  resourceType: "key",
};
```

## Fields

| Field                     | Type                      | Required                  | Description               |
| ------------------------- | ------------------------- | ------------------------- | ------------------------- |
| `data`                    | *models.KeyHeartbeatData* | :heavy_check_mark:        | N/A                       |
| `resourceType`            | *"key"*                   | :heavy_check_mark:        | N/A                       |