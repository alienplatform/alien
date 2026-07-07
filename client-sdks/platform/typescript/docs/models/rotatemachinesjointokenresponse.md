# RotateMachinesJoinTokenResponse

## Example Usage

```typescript
import { RotateMachinesJoinTokenResponse } from "@alienplatform/platform-api/models";

let value: RotateMachinesJoinTokenResponse = {
  joinToken: "<value>",
  controlPlaneUrl: "https://untried-harp.net",
  clusterId: "<id>",
  token: {
    id: "<id>",
    createdAt: "1705214613068",
    createdBy: "<value>",
    joinCount: 219120,
  },
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `joinToken`                                                              | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `controlPlaneUrl`                                                        | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `clusterId`                                                              | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `token`                                                                  | [models.MachinesJoinTokenSummary](../models/machinesjointokensummary.md) | :heavy_check_mark:                                                       | N/A                                                                      |