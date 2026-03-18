# SyncReconcileRequestPreparedStackOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncReconcileRequestPreparedStackOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestPreparedStackOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `condition`                                                           | *models.SyncReconcileRequestPreparedStackOverrideStackConditionUnion* | :heavy_minus_sign:                                                    | N/A                                                                   |
| `scope`                                                               | *string*                                                              | :heavy_check_mark:                                                    | Scope (project/resource level)                                        |