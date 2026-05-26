# SyncAcquireResponseHorizonMachineImageAws

AWS Horizon machine image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseHorizonMachineImageAws } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizonMachineImageAws = {
  amis: {
    "key": {},
    "key1": {
      "key": "<value>",
    },
  },
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `amis`                                    | Record<string, Record<string, *string*>>  | :heavy_check_mark:                        | AMI IDs by architecture, then AWS region. |