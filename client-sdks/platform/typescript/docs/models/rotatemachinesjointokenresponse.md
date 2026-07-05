# RotateMachinesJoinTokenResponse

## Example Usage

```typescript
import { RotateMachinesJoinTokenResponse } from "@alienplatform/platform-api/models";

let value: RotateMachinesJoinTokenResponse = {
  joinToken: "<value>",
  token: {
    id: "<id>",
    createdAt: "1728815037714",
    createdBy: "<value>",
    joinCount: 907462,
  },
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `joinToken`                                                              | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `token`                                                                  | [models.MachinesJoinTokenSummary](../models/machinesjointokensummary.md) | :heavy_check_mark:                                                       | N/A                                                                      |