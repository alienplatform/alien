# CreateReleaseRequest

## Example Usage

```typescript
import { CreateReleaseRequest } from "@alienplatform/platform-api/models";

let value: CreateReleaseRequest = {
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
  stack: {},
  project: "<value>",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `gitMetadata`                                          | [models.GitMetadata](../models/gitmetadata.md)         | :heavy_minus_sign:                                     | N/A                                                    |
| `stack`                                                | [models.StackByPlatform](../models/stackbyplatform.md) | :heavy_check_mark:                                     | N/A                                                    |
| `rootDirectory`                                        | *string*                                               | :heavy_minus_sign:                                     | N/A                                                    |
| `project`                                              | *string*                                               | :heavy_check_mark:                                     | Project ID or name                                     |