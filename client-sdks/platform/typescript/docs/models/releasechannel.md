# ReleaseChannel

## Example Usage

```typescript
import { ReleaseChannel } from "@alienplatform/platform-api/models";

let value: ReleaseChannel = {
  workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
  projectId: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
  name: "<value>",
  currentReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
  createdAt: new Date("2026-06-15T04:15:50.307Z"),
  updatedAt: new Date("2024-10-13T06:24:32.093Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   | Example                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `workspaceId`                                                                                 | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the workspace.                                                          | ws_It13CUaGEhLLAB87simX07Xb                                                                   |
| `projectId`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the project.                                                            | prj_mcytp6z3j91f7tn5ryqsfwtrdqkv                                                              |
| `name`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | Channel the release was signed for                                                            |                                                                                               |
| `currentReleaseId`                                                                            | *string*                                                                                      | :heavy_check_mark:                                                                            | Unique identifier for the release.                                                            | rel_WbhQgksrawSKIpEN0NAssHX9E2Su                                                              |
| `deploymentCount`                                                                             | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |                                                                                               |
| `currentReleaseCount`                                                                         | *number*                                                                                      | :heavy_minus_sign:                                                                            | Of the deployments following this channel, how many run its current release                   |                                                                                               |
| `pinnedCount`                                                                                 | *number*                                                                                      | :heavy_minus_sign:                                                                            | Of the deployments following this channel, how many are pinned to a release                   |                                                                                               |
| `createdAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |
| `updatedAt`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |                                                                                               |