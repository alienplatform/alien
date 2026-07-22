# PendingPreparedStackProfileStateGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PendingPreparedStackProfileStateGcpResource } from "@alienplatform/platform-api/models";

let value: PendingPreparedStackProfileStateGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `condition`                                                     | *models.PendingPreparedStackProfileStateResourceConditionUnion* | :heavy_minus_sign:                                              | N/A                                                             |
| `scope`                                                         | *string*                                                        | :heavy_check_mark:                                              | Scope (project/resource level)                                  |
