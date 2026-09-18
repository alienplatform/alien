# TargetDeploymentHorizonMachineImageAws

AWS Horizon machine image catalog.

## Example Usage

```typescript
import { TargetDeploymentHorizonMachineImageAws } from "@alienplatform/platform-api/models";

let value: TargetDeploymentHorizonMachineImageAws = {
  amis: {
    "key": {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
    "key1": {
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