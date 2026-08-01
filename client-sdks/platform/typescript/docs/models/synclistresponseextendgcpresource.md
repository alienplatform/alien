# SyncListResponseExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { SyncListResponseExtendGcpResource } from "@alienplatform/platform-api/models";

let value: SyncListResponseExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `condition`                                           | *models.SyncListResponseExtendResourceConditionUnion* | :heavy_minus_sign:                                    | N/A                                                   |
| `scope`                                               | *string*                                              | :heavy_check_mark:                                    | Scope (project/resource level)                        |