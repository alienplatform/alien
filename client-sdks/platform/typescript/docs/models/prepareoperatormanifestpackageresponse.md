# PrepareOperatorManifestPackageResponse

## Example Usage

```typescript
import { PrepareOperatorManifestPackageResponse } from "@alienplatform/platform-api/models";

let value: PrepareOperatorManifestPackageResponse = {
  package: {
    id: "pkg_jebo2o5jmm7raefl2m1pe3czjjq4",
    projectId: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
    workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
    type: "cli",
    status: "pending",
    version: "<value>",
    sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
    dependsOnPackageId: "pkg_jebo2o5jmm7raefl2m1pe3czjjq4",
    setupFingerprints: {
      "key": {
        target: "<value>",
        fingerprint: "<value>",
        version: 76165,
      },
    },
    packageBuildInputHash: "<value>",
    config: {
      agentImage: "<value>",
      baseImage: "<value>",
      customImage: "<value>",
      managerUrl: "https://french-roadway.org/",
      type: "gcp-sandbox-image",
    },
    retries: 386232,
    createdAt: new Date("2025-05-20T06:05:48.607Z"),
    updatedAt: new Date("2026-02-26T09:32:51.356Z"),
  },
};
```

## Fields

| Field                                  | Type                                   | Required                               | Description                            |
| -------------------------------------- | -------------------------------------- | -------------------------------------- | -------------------------------------- |
| `package`                              | [models.Package](../models/package.md) | :heavy_check_mark:                     | N/A                                    |