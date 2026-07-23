# SyncListResponsePreparedStackEnv

How a resolved stack input is injected into runtime environment variables.

## Example Usage

```typescript
import { SyncListResponsePreparedStackEnv } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackEnv = {
  name: "<value>",
};
```

## Fields

| Field                                                                   | Type                                                                    | Required                                                                | Description                                                             |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `name`                                                                  | *string*                                                                | :heavy_check_mark:                                                      | Environment variable name.                                              |
| `targetResources`                                                       | *string*[]                                                              | :heavy_minus_sign:                                                      | Target resource IDs or patterns. None means every env-capable resource. |
| `type`                                                                  | *models.SyncListResponsePreparedStackTypeUnion*                         | :heavy_minus_sign:                                                      | N/A                                                                     |
