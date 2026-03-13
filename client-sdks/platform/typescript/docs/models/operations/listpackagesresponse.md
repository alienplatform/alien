# ListPackagesResponse

Paginated response

## Example Usage

```typescript
import { ListPackagesResponse } from "@aliendotdev/platform-api/models/operations";

let value: ListPackagesResponse = {
  items: [
    {
      id: "pkg_jebo2o5jmm7raefl2m1pe3cz",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
      workspaceId: "ws_It13CUaGEhLLAB87simX0",
      type: "operator-image",
      status: "failed",
      version: "<value>",
      config: {
        type: "cloudformation",
      },
      retries: 182148,
      createdAt: new Date("2026-11-24T17:48:56.233Z"),
      updatedAt: new Date("2025-04-14T11:51:09.728Z"),
    },
  ],
  nextCursor: "<value>",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `items`                                     | [models.Package](../../models/package.md)[] | :heavy_check_mark:                          | Items in this page                          |
| `nextCursor`                                | *string*                                    | :heavy_check_mark:                          | Cursor for the next page, null if last page |