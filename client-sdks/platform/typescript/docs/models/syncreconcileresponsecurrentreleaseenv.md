# SyncReconcileResponseCurrentReleaseEnv

How a resolved stack input is injected into runtime environment variables.

## Example Usage

```typescript
import { SyncReconcileResponseCurrentReleaseEnv } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCurrentReleaseEnv = {
  name: "<value>",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `name`                                                                  | *string*                                                                | :heavy_check_mark:                                                      | Environment variable name.                                              |
| `targetResources`                                                       | *string*[]                                                              | :heavy_minus_sign:                                                      | Target resource IDs or patterns. None means every env-capable resource. |
| `type`                                                                  | *models.SyncReconcileResponseCurrentReleaseTypeUnion*                   | :heavy_minus_sign:                                                      | N/A                                                                     |