# MachineOsUpdateDone

## Example Usage

```typescript
import { MachineOsUpdateDone } from "@alienplatform/platform-api/models";

let value: MachineOsUpdateDone = {
  state: "done",
  imageVersion: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `state`                                                                  | [models.MachineOsUpdateDoneState](../models/machineosupdatedonestate.md) | :heavy_check_mark:                                                       | N/A                                                                      |
| `imageVersion`                                                           | *string*                                                                 | :heavy_check_mark:                                                       | Machine image version the deployment's channel publishes.                |