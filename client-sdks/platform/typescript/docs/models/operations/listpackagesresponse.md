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
      type: "agent-image",
      status: "failed",
      version: "<value>",
      sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      setupFingerprints: {},
      packageGeneratorContractVersion: "<value>",
      config: {
        displayName: "Virginia51",
        name: "<value>",
        type: "cli",
      },
      retries: 480954,
      createdAt: new Date("2024-03-13T23:16:16.358Z"),
      updatedAt: new Date("2024-08-14T21:19:41.899Z"),
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