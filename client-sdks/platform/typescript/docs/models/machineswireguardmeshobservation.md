# MachinesWireGuardMeshObservation

## Example Usage

```typescript
import { MachinesWireGuardMeshObservation } from "@alienplatform/platform-api/models";

let value: MachinesWireGuardMeshObservation = {
  expectedPeerCount: 508256,
  reachablePeerCount: 272409,
  missingPeerMachineIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `expectedPeerCount`     | *number*                | :heavy_check_mark:      | N/A                     |
| `reachablePeerCount`    | *number*                | :heavy_check_mark:      | N/A                     |
| `missingPeerMachineIds` | *string*[]              | :heavy_check_mark:      | N/A                     |