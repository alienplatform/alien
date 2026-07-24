# SyncReconcileResponsePendingPreparedStackEnv

How a resolved stack input is injected into runtime environment variables.

## Example Usage

```typescript
import { SyncReconcileResponsePendingPreparedStackEnv } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePendingPreparedStackEnv = {
  name: "<value>",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `name`                                                                  | *string*                                                                | :heavy_check_mark:                                                      | Environment variable name.                                              |
| `targetResources`                                                       | *string*[]                                                              | :heavy_minus_sign:                                                      | Target resource IDs or patterns. None means every env-capable resource. |
| `type`                                                                  | *models.SyncReconcileResponsePendingPreparedStackTypeUnion*             | :heavy_minus_sign:                                                      | N/A                                                                     |
