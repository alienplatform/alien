# ListPackagesResponse

Paginated response

## Example Usage

```typescript
import { ListPackagesResponse } from "@alienplatform/platform-api/models/operations";

let value: ListPackagesResponse = {
  items: [
    {
      id: "pkg_jebo2o5jmm7raefl2m1pe3cz",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
      workspaceId: "ws_It13CUaGEhLLAB87simX0",
      type: "operator-image",
      status: "failed",
      version: "<value>",
      sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      setupFingerprints: {},
      packageBuildInputHash: "<value>",
      config: {
        type: "cloudformation",
      },
      retries: 966006,
      createdAt: new Date("2025-04-14T11:51:09.728Z"),
      updatedAt: new Date("2024-08-25T22:26:49.704Z"),
    },
  ],
  nextCursor: null,
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `items`                                     | [models.Package](../../models/package.md)[] | :heavy_check_mark:                          | Items in this page                          |
| `nextCursor`                                | *string*                                    | :heavy_check_mark:                          | Cursor for the next page, null if last page |