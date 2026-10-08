# WireguardMesh

## Example Usage

```typescript
import { WireguardMesh } from "@alienplatform/platform-api/models";

let value: WireguardMesh = {
  expectedPeerCount: 343268,
  reachablePeerCount: 432188,
  missingPeerMachineIds: [],
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `expectedPeerCount`     | *number*                | :heavy_check_mark:      | N/A                     |
| `reachablePeerCount`    | *number*                | :heavy_check_mark:      | N/A                     |
| `missingPeerMachineIds` | *string*[]              | :heavy_check_mark:      | N/A                     |