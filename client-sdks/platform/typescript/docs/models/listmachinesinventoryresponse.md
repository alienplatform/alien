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
      drainBlockers: [
        {
          reason: "<value>",
        },
      ],
      drainForce: false,
      lastHeartbeat: "<value>",
      replicaCount: 999417,
    },
  ],
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `machines`                                                           | [models.MachinesInventoryItem](../models/machinesinventoryitem.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |