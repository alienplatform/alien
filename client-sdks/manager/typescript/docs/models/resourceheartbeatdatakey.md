# ResourceHeartbeatDataKey

## Example Usage

```typescript
import { ResourceHeartbeatDataKey } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataKey = {
  data: {
    data: {
      enabled: true,
      keyArn: "<value>",
      keySpec: "<value>",
      keyState: "<value>",
      keyUsage: "<value>",
      status: {
        health: "healthy",
        lifecycle: "running",
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