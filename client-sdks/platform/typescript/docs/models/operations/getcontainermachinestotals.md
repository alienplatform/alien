# GetContainerMachinesTotals

## Example Usage

```typescript
import { GetContainerMachinesTotals } from "@aliendotdev/platform-api/models/operations";

let value: GetContainerMachinesTotals = {
  machines: 649274,
  machinesByStatus: {
    running: 941694,
    unhealthy: 424809,
    initializing: 323712,
    draining: 542030,
  },
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `machines`                                                                                                                     | *number*                                                                                                                       | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |
| `machinesByStatus`                                                                                                             | [operations.GetContainerMachinesTotalsMachinesByStatus](../../models/operations/getcontainermachinestotalsmachinesbystatus.md) | :heavy_check_mark:                                                                                                             | N/A                                                                                                                            |