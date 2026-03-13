# GitNamespace

## Example Usage

```typescript
import { GitNamespace } from "@alienplatform/platform-api/models";

let value: GitNamespace = {
  id: 5821.47,
  name: "<value>",
  slug: "<value>",
  installationId: null,
  type: "team",
  provider: "github",
  createdAt: new Date("2024-03-24T11:20:07.125Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `slug`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `installationId`                                                                              | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `type`                                                                                        | [models.GitNamespaceType](../models/gitnamespacetype.md)                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `provider`                                                                                    | [models.Provider](../models/provider.md)                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |