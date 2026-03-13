# Release

## Example Usage

```typescript
import { Release } from "@aliendotdev/platform-api/models";

let value: Release = {
  id: "rel_WbhQgksrawSKIpEN0NAssHX9",
  projectId: "<id>",
  gitMetadata: {
    commitSha: "dc36199b2234c6586ebe05ec94078a895c707e29",
    commitMessage:
      "add method to measure Interaction to Next Paint (INP) (#36490)",
    commitRef: "main",
    commitDate: new Date("2025-09-29T12:00:00Z"),
    dirty: true,
    remoteUrl: "https://github.com/alienplatform/alien",
    commitAuthorName: "John Doe",
    commitAuthorEmail: "john@example.com",
    commitAuthorLogin: "johndoe",
    commitAuthorAvatarUrl: "https://github.com/johndoe.png",
  },
  createdAt: new Date("2026-08-08T13:05:28.803Z"),
  stack: {},
  workspaceId: "<id>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `id`                                                                                          | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the release.                                                            | rel_WbhQgksrawSKIpEN0NAssHX9                                                                  |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `gitMetadata`                                                                                 | [models.GitMetadata](../models/gitmetadata.md)                                                | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `stack`                                                                                       | [models.StackByPlatform](../models/stackbyplatform.md)                                        | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `rootDirectory`                                                                               | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |