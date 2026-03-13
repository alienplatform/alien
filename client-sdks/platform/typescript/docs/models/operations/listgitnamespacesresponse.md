# ListGitNamespacesResponse

List of user's git namespaces.

## Example Usage

```typescript
import { ListGitNamespacesResponse } from "@aliendotdev/platform-api/models/operations";

let value: ListGitNamespacesResponse = {
  items: [
    {
      id: 7786.12,
      name: "<value>",
      slug: "<value>",
      installationId: 3535.6,
      type: "user",
      provider: "github",
      createdAt: new Date("2026-03-15T05:02:24.915Z"),
    },
  ],
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `items`                                               | [models.GitNamespace](../../models/gitnamespace.md)[] | :heavy_check_mark:                                    | N/A                                                   |