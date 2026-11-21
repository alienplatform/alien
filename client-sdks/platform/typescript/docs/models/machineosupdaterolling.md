# MachineOsUpdateRolling

## Example Usage

```typescript
import { MachineOsUpdateRolling } from "@alienplatform/platform-api/models";

let value: MachineOsUpdateRolling = {
  state: "rolling",
  imageVersion: "<value>",
  remaining: 289661,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `state`                                                                        | [models.MachineOsUpdateRollingState](../models/machineosupdaterollingstate.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `imageVersion`                                                                 | *string*                                                                       | :heavy_check_mark:                                                             | Machine image version the deployment's channel publishes.                      |
| `remaining`                                                                    | *number*                                                                       | :heavy_check_mark:                                                             | Machines still to replace.                                                     |
| `total`                                                                        | *number*                                                                       | :heavy_minus_sign:                                                             | Machines the roll covers; absent for a roll started before it was recorded.    |
| `replaced`                                                                     | *number*                                                                       | :heavy_minus_sign:                                                             | Machines already replaced; present with `total`.                               |
| `reason`                                                                       | *string*                                                                       | :heavy_minus_sign:                                                             | What the next replacement waits for, when known.                               |