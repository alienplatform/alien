# MachinesInventoryItem

## Example Usage

```typescript
import { MachinesInventoryItem } from "@alienplatform/platform-api/models";

let value: MachinesInventoryItem = {
  machineId: "<id>",
  status: "<value>",
  capacityGroup: "<value>",
  zone: "<value>",
  lastHeartbeat: "<value>",
  replicaCount: 173601,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `machineId`        | *string*           | :heavy_check_mark: | N/A                |
| `status`           | *string*           | :heavy_check_mark: | N/A                |
| `capacityGroup`    | *string*           | :heavy_check_mark: | N/A                |
| `zone`             | *string*           | :heavy_check_mark: | N/A                |
| `publicIp`         | *string*           | :heavy_minus_sign: | N/A                |
| `overlayIp`        | *string*           | :heavy_minus_sign: | N/A                |
| `lastHeartbeat`    | *string*           | :heavy_check_mark: | N/A                |
| `horizondVersion`  | *string*           | :heavy_minus_sign: | N/A                |
| `replicaCount`     | *number*           | :heavy_check_mark: | N/A                |