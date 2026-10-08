# ManagerCapabilities

## Example Usage

```typescript
import { ManagerCapabilities } from "@alienplatform/manager-api/models";

let value: ManagerCapabilities = {
  charts: true,
  tunnels: false,
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `charts`                                                              | *boolean*                                                             | :heavy_check_mark:                                                    | Helm charts at `oci://<registryHost>/charts/<stack>`.                 |
| `tunnels`                                                             | *boolean*                                                             | :heavy_check_mark:                                                    | Requests into deployments through `/v1/deployments/{id}/tunnels/...`. |