# SyncAcquireResponseCurrentReleaseEnv

How a resolved stack input is injected into runtime environment variables.

## Example Usage

```typescript
import { SyncAcquireResponseCurrentReleaseEnv } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCurrentReleaseEnv = {
  name: "<value>",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `name`                                                                  | *string*                                                                | :heavy_check_mark:                                                      | Environment variable name.                                              |
| `targetResources`                                                       | *string*[]                                                              | :heavy_minus_sign:                                                      | Target resource IDs or patterns. None means every env-capable resource. |
| `type`                                                                  | *models.SyncAcquireResponseCurrentReleaseTypeUnion*                     | :heavy_minus_sign:                                                      | N/A                                                                     |