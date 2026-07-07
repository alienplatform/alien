# ListMachinesInventoryResponse

## Example Usage

```typescript
import { ListMachinesInventoryResponse } from "@alienplatform/platform-api/models";

let value: ListMachinesInventoryResponse = {
  machines: [
    {
      machineId: "<id>",
      status: "<value>",
      capacityGroup: "<value>",
      zone: "<value>",
      cpu: {
        allocated: 9516.82,
        systemReserve: 7650.68,
        total: 9994.17,
      },
      memory: {
        allocated: 4854.69,
        systemReserve: 2953.4,
        total: 9773.24,
      },
      drainBlockers: [
        {
          reason: "<value>",
        },
      ],
      drainForce: false,
      lastHeartbeat: "<value>",
      replicaCount: 716534,
    },
  ],
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `machines`                                                           | [models.MachinesInventoryItem](../models/machinesinventoryitem.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |