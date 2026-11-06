# ListEventsResponse

Retrieved events.

## Example Usage

```typescript
import { ListEventsResponse } from "@alienplatform/platform-api/models/operations";

let value: ListEventsResponse = {
  items: [
    {
      id: "event_MtSA24M3pWuAkQYxgZxuRIg6I5i2",
      deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlcxd7f",
      releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
      debugSessionId: "dbg_HOXmkmT9UPYlsnxqSNlEGoXLGcNV",
      data: {
        nextState: {
          platform: "test",
          resourcePrefix: "<value>",
          resources: {
            "key": {
              config: {
                id: "<id>",
                type: "<value>",
              },
              status: "delete-failed",
              type: "<value>",
            },
          },
        },
        type: "StackStep",
      },
      state: "none",
      projectId: "prj_mcytp6z3j91f7tn5ryqsfwtrdqkv",
      createdAt: new Date("2026-05-23T10:59:53.935Z"),
      workspaceId: "ws_It13CUaGEhLLAB87simX07Xb",
    },
  ],
  nextCursor: "<value>",
  releases: [
    {
      id: "rel_WbhQgksrawSKIpEN0NAssHX9E2Su",
      version: "<value>",
      gitMetadata: {
        commitSha: "dc36199b2234c6586ebe05ec94078a895c707e29",
        commitMessage:
          "add method to measure Interaction to Next Paint (INP) (#36490)",
        commitRef: "main",
        commitDate: new Date("2026-03-16T12:00:00Z"),
        dirty: true,
        remoteUrl: "https://github.com/alienplatform/alien",
        commitAuthorName: "John Doe",
        commitAuthorEmail: "john@example.com",
        commitAuthorLogin: "johndoe",
        commitAuthorAvatarUrl: "https://github.com/johndoe.png",
      },
      createdAt: new Date("2026-04-13T18:46:22.262Z"),
    },
  ],
};
```

## Fields

| Field                                                                                                                                                                                        | Type                                                                                                                                                                                         | Required                                                                                                                                                                                     | Description                                                                                                                                                                                  |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `items`                                                                                                                                                                                      | [models.EventListItemResponse](../../models/eventlistitemresponse.md)[]                                                                                                                      | :heavy_check_mark:                                                                                                                                                                           | Items in this page                                                                                                                                                                           |
| `nextCursor`                                                                                                                                                                                 | *string*                                                                                                                                                                                     | :heavy_check_mark:                                                                                                                                                                           | Cursor for the next page, null if last page                                                                                                                                                  |
| `cloudAccounts`                                                                                                                                                                              | [operations.CloudAccount](../../models/operations/cloudaccount.md)[]                                                                                                                         | :heavy_minus_sign:                                                                                                                                                                           | Subaccounts referenced by events, with ?include=cloudAccounts                                                                                                                                |
| `deployments`                                                                                                                                                                                | [operations.ListEventsDeployment](../../models/operations/listeventsdeployment.md)[]                                                                                                         | :heavy_minus_sign:                                                                                                                                                                           | Deployments the listed events reference, with ?include=deployments                                                                                                                           |
| `releases`                                                                                                                                                                                   | [operations.ListEventsRelease](../../models/operations/listeventsrelease.md)[]                                                                                                               | :heavy_minus_sign:                                                                                                                                                                           | Releases the listed events reference, with ?include=releases                                                                                                                                 |
| `actors`                                                                                                                                                                                     | [operations.Actor](../../models/operations/actor.md)[]                                                                                                                                       | :heavy_minus_sign:                                                                                                                                                                           | People the listed events name as their actor, with their display name and avatar, with ?include=actors                                                                                       |
| `apiKeys`                                                                                                                                                                                    | [operations.ApiKey](../../models/operations/apikey.md)[]                                                                                                                                     | :heavy_minus_sign:                                                                                                                                                                           | API keys the listed events name as their actor, by name, with ?include=actors; only keys the caller can read                                                                                 |
| `rollouts`                                                                                                                                                                                   | [operations.ListEventsRollout](../../models/operations/listeventsrollout.md)[]                                                                                                               | :heavy_minus_sign:                                                                                                                                                                           | For each release the listed DeploymentReleased events name, how many deployments it reached on each of their UTC days, counted over all events rather than this page, with ?include=rollouts |