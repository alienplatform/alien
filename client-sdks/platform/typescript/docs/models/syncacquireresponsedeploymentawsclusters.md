# SyncAcquireResponseDeploymentAwsClusters

AWS machine image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentAwsClusters } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentAwsClusters = {
  amis: {
    "key": {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
  },
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `amis`                                    | Record<string, Record<string, *string*>>  | :heavy_check_mark:                        | AMI IDs by architecture, then AWS region. |