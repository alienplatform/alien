# DeploymentMachinesByStatus

## Example Usage

```typescript
import { DeploymentMachinesByStatus } from "@aliendotdev/platform-api/models/operations";

let value: DeploymentMachinesByStatus = {
  running: 987536,
  unhealthy: 608912,
  initializing: 638074,
  draining: 310745,
};
```

## Fields

| Field              | Type               | Required           | Description        |
| ------------------ | ------------------ | ------------------ | ------------------ |
| `running`          | *number*           | :heavy_check_mark: | N/A                |
| `unhealthy`        | *number*           | :heavy_check_mark: | N/A                |
| `initializing`     | *number*           | :heavy_check_mark: | N/A                |
| `draining`         | *number*           | :heavy_check_mark: | N/A                |