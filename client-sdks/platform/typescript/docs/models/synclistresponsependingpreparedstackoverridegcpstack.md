# SyncListResponsePendingPreparedStackOverrideGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackOverrideGcpStack } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackOverrideGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `condition`                                                              | *models.SyncListResponsePendingPreparedStackOverrideStackConditionUnion* | :heavy_minus_sign:                                                       | N/A                                                                      |
| `scope`                                                                  | *string*                                                                 | :heavy_check_mark:                                                       | Scope (project/resource level)                                           |
