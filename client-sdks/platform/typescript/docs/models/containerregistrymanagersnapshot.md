# ContainerRegistryManagerSnapshot

## Example Usage

```typescript
import { ContainerRegistryManagerSnapshot } from "@alienplatform/platform-api/models";

let value: ContainerRegistryManagerSnapshot = {
  generatedAt: new Date("2024-05-29T08:29:14.011Z"),
  routes: [
    {
      id: "crroute_c9t4xoy7fmiq7equtu20",
      deploymentId: "<id>",
      resourceId: "<id>",
      resourceRevision: "<value>",
      desiredRevision: 618205,
      appliedRevision: 940160,
      status: "connected",
      repositories: [],
      credentials: [
        {
          id: "crcred_oz1xjr82f37j17g4gtmyu",
          secretPrefix: "<value>",
          secretDigest: "<value>",
          scope: "pushPull",
          repositorySubset: [
            "<value 1>",
            "<value 2>",
          ],
          expiresAt: new Date("2026-06-28T15:05:52.870Z"),
          revokedAt: new Date("2025-03-04T20:06:17.536Z"),
        },
      ],
    },
  ],
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `generatedAt`                                                                                        | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)        | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `routes`                                                                                             | [models.ContainerRegistryManagerSnapshotRoute](../models/containerregistrymanagersnapshotroute.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |