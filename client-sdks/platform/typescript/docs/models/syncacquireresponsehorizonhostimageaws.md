# SyncAcquireResponseHorizonHostImageAws

AWS Horizon host image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseHorizonHostImageAws } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizonHostImageAws = {
  amis: {
    "key": {
      "key": "<value>",
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