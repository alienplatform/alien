# ProviderFleetStatus

## Example Usage

```typescript
import { ProviderFleetStatus } from "@alienplatform/manager-api/models";

let value: ProviderFleetStatus = {
  currentMachines: 877172,
  desiredMachines: 582194,
  groupId: "<id>",
  providerId: "<id>",
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `currentMachines`  | *number*           | :heavy_check_mark: | N/A                |
| `desiredMachines`  | *number*           | :heavy_check_mark: | N/A                |
| `groupId`          | *string*           | :heavy_check_mark: | N/A                |
| `location`         | *string*           | :heavy_minus_sign: | N/A                |
| `providerId`       | *string*           | :heavy_check_mark: | N/A                |