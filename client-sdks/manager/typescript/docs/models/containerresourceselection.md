# ContainerResourceSelection

Deployment-time resource allocation for a container. Omitted fields use release defaults.

## Example Usage

```typescript
import { ContainerResourceSelection } from "@alienplatform/manager-api/models";

let value: ContainerResourceSelection = {};
```

## Fields

| Field                                                                                                                         | Type                                                                                                                          | Required                                                                                                                      | Description                                                                                                                   |
| ----------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `cpu`                                                                                                                         | *number*                                                                                                                      | :heavy_minus_sign:                                                                                                            | CPU allocation in vCPUs.                                                                                                      |
| `memory`                                                                                                                      | *string*                                                                                                                      | :heavy_minus_sign:                                                                                                            | Memory allocation, using binary units such as Mi or Gi.                                                                       |
| `volumeSizeGib`                                                                                                               | *number*                                                                                                                      | :heavy_minus_sign:                                                                                                            | Persistent volume capacity per replica, in whole GiB. Omitted values<br/>use the release default. Existing volumes can only grow. |