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

| Field                                                                                                                                     | Type                                                                                                                                      | Required                                                                                                                                  | Description                                                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `charts`                                                                                                                                  | *boolean*                                                                                                                                 | :heavy_check_mark:                                                                                                                        | Helm charts at `oci://<registryHost>/charts/<stack>`.                                                                                     |
| `tunnels`                                                                                                                                 | *boolean*                                                                                                                                 | :heavy_check_mark:                                                                                                                        | Always false: container tunnels were removed. Still served because<br/>CLIs released before the removal require the field.                |
| `values`                                                                                                                                  | *number*                                                                                                                                  | :heavy_minus_sign:                                                                                                                        | Deployment values held by this manager at `/v1/values/{scope}`. The<br/>number is the version of that API; absent when values are not served. |
