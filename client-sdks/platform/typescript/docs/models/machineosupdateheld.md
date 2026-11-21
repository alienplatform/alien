# MachineOsUpdateHeld

## Example Usage

```typescript
import { MachineOsUpdateHeld } from "@alienplatform/platform-api/models";

let value: MachineOsUpdateHeld = {
  state: "held",
  imageVersion: "<value>",
  reason: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `state`                                                                  | [models.MachineOsUpdateHeldState](../models/machineosupdateheldstate.md) | :heavy_check_mark:                                                       | N/A                                                                      |
| `imageVersion`                                                           | *string*                                                                 | :heavy_check_mark:                                                       | Machine image version the deployment's channel publishes.                |
| `reason`                                                                 | *string*                                                                 | :heavy_check_mark:                                                       | Why the machines are not moving to the channel's image.                  |