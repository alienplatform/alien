# CreateProjectBuckets

## Example Usage

```typescript
import { CreateProjectBuckets } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectBuckets = {
  enabled: false,
  access: "read-write",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `enabled`                                                                        | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `access`                                                                         | [operations.CreateProjectAccess](../../models/operations/createprojectaccess.md) | :heavy_check_mark:                                                               | N/A                                                                              |