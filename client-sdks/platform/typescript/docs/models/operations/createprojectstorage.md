# CreateProjectStorage

## Example Usage

```typescript
import { CreateProjectStorage } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectStorage = {
  enabled: false,
  access: "read-write",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `enabled`                                                                        | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `access`                                                                         | [operations.CreateProjectAccess](../../models/operations/createprojectaccess.md) | :heavy_check_mark:                                                               | N/A                                                                              |