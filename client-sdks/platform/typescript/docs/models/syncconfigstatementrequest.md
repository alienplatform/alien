# SyncConfigStatementRequest

## Example Usage

```typescript
import { SyncConfigStatementRequest } from "@alienplatform/platform-api/models";

let value: SyncConfigStatementRequest = {
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5tv5y",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           | Example                               |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `managerId`                           | *string*                              | :heavy_minus_sign:                    | Unique identifier for a manager.      | mgr_enxscjrqiiu2lrc672hwwuc5tv5y      |
| `deploymentId`                        | *string*                              | :heavy_check_mark:                    | Unique identifier for the deployment. | dep_0c29fq4a2yjb7kx3smwdgxlcxd7f      |