# PrepareOperatorManifestPackageResponse

## Example Usage

```typescript
import { PrepareOperatorManifestPackageResponse } from "@alienplatform/platform-api/models";

let value: PrepareOperatorManifestPackageResponse = {
  package: {
    id: "pkg_jebo2o5jmm7raefl2m1pe3cz",
    projectId: "prj_mcytp6z3j91f7tn5ryqsfwtr",
    workspaceId: "ws_It13CUaGEhLLAB87simX0",
    type: "cli",
    status: "pending",
    version: "<value>",
    sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    setupFingerprints: {
      "key": {
        target: "<value>",
        fingerprint: "<value>",
        version: 76165,
      },
    },
    packageBuildInputHash: "<value>",
    config: {
      type: "cloudformation",
    },
    retries: 147469,
    createdAt: new Date("2024-11-19T20:57:14.511Z"),
    updatedAt: new Date("2026-04-15T04:09:13.284Z"),
  },
};
```

## Fields

| Field                                  | Type                                   | Required                               | Description                            |
| -------------------------------------- | -------------------------------------- | -------------------------------------- | -------------------------------------- |
| `package`                              | [models.Package](../models/package.md) | :heavy_check_mark:                     | N/A                                    |