# SyncAcquireResponsePreparedStackProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackProfileGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePreparedStackProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `condition`                                                         | *models.SyncAcquireResponsePreparedStackProfileStackConditionUnion* | :heavy_minus_sign:                                                  | N/A                                                                 |
| `scope`                                                             | *string*                                                            | :heavy_check_mark:                                                  | Scope (project/resource level)                                      |