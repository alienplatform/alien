# SyncListResponseContainers

Deployment-time resource allocation for a container. Omitted fields use release defaults.

## Example Usage

```typescript
import { SyncListResponseContainers } from "@alienplatform/platform-api/models";

let value: SyncListResponseContainers = {};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `cpu`                                                   | *number*                                                | :heavy_minus_sign:                                      | CPU allocation in vCPUs.                                |
| `memory`                                                | *string*                                                | :heavy_minus_sign:                                      | Memory allocation, using binary units such as Mi or Gi. |
