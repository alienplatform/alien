# TargetDynamicContainer

## Example Usage

```typescript
import { TargetDynamicContainer } from "@alienplatform/platform-api/models";

let value: TargetDynamicContainer = {
  name: "<value>",
  generation: 361562,
  image: "https://picsum.photos/seed/SSL54/7/408",
  cpu: "<value>",
  memory: "<value>",
  replicas: 271152,
  ports: [],
  deleted: true,
  env: {
    "key": "<value>",
    "key1": "<value>",
    "key2": "<value>",
  },
  secretEnv: {
    "key": "<value>",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `name`                                                                                   | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `generation`                                                                             | *number*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `image`                                                                                  | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `cpu`                                                                                    | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `memory`                                                                                 | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `replicas`                                                                               | *number*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `ports`                                                                                  | *number*[]                                                                               | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `deleted`                                                                                | *boolean*                                                                                | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `env`                                                                                    | Record<string, *string*>                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `secretEnv`                                                                              | Record<string, *string*>                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `healthCheck`                                                                            | [models.SyncReconcileResponseHealthCheck](../models/syncreconcileresponsehealthcheck.md) | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `suspendedReason`                                                                        | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |