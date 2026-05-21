# SyncListResponseOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncListResponseOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: SyncListResponseOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `condition`                                             | *models.SyncListResponseOverrideResourceConditionUnion* | :heavy_minus_sign:                                      | N/A                                                     |
| `scope`                                                 | *string*                                                | :heavy_check_mark:                                      | Scope (project/resource level)                          |