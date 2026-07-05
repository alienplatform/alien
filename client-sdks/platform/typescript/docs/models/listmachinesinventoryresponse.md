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
      lastHeartbeat: "<value>",
      replicaCount: 951682,
    },
  ],
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `machines`                                                           | [models.MachinesInventoryItem](../models/machinesinventoryitem.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |