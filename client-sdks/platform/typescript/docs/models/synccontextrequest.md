# SyncContextRequest

## Example Usage

```typescript
import { SyncContextRequest } from "@alienplatform/platform-api/models";

let value: SyncContextRequest = {
  managerId: "mgr_enxscjrqiiu2lrc672hwwuc5",
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           | Example                               |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `managerId`                           | *string*                              | :heavy_minus_sign:                    | N/A                                   | mgr_enxscjrqiiu2lrc672hwwuc5          |
| `deploymentId`                        | *string*                              | :heavy_check_mark:                    | Unique identifier for the deployment. | dep_0c29fq4a2yjb7kx3smwdgxlc          |