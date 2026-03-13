# GetContainerOverviewMachinesByStatus

## Example Usage

```typescript
import { GetContainerOverviewMachinesByStatus } from "@alienplatform/platform-api/models/operations";

let value: GetContainerOverviewMachinesByStatus = {
  running: 838099,
  unhealthy: 394343,
  initializing: 22599,
  draining: 139717,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `running`          | *number*           | :heavy_check_mark: | N/A                |
| `unhealthy`        | *number*           | :heavy_check_mark: | N/A                |
| `initializing`     | *number*           | :heavy_check_mark: | N/A                |
| `draining`         | *number*           | :heavy_check_mark: | N/A                |