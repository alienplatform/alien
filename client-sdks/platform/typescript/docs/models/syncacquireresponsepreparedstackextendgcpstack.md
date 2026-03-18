# SyncAcquireResponsePreparedStackExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackExtendGcpStack } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePreparedStackExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `condition`                                                        | *models.SyncAcquireResponsePreparedStackExtendStackConditionUnion* | :heavy_minus_sign:                                                 | N/A                                                                |
| `scope`                                                            | *string*                                                           | :heavy_check_mark:                                                 | Scope (project/resource level)                                     |