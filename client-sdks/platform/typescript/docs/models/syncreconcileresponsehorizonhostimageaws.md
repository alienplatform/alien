# SyncReconcileResponseHorizonHostImageAws

AWS Horizon host image catalog.

## Example Usage

```typescript
import { SyncReconcileResponseHorizonHostImageAws } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseHorizonHostImageAws = {
  amis: {
    "key": {},
    "key1": {},
  },
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `amis`                                    | Record<string, Record<string, *string*>>  | :heavy_check_mark:                        | AMI IDs by architecture, then AWS region. |