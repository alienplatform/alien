# GitHubProvider

## Example Usage

```typescript
import { GitHubProvider } from "@aliendotdev/platform-api/models";

let value: GitHubProvider = {
  type: "github",
  org: "<value>",
  repo: "<value>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `type`                                                       | [models.GitHubProviderType](../models/githubprovidertype.md) | :heavy_check_mark:                                           | N/A                                                          |
| `org`                                                        | *string*                                                     | :heavy_check_mark:                                           | Repository owner (user or organization)                      |
| `repo`                                                       | *string*                                                     | :heavy_check_mark:                                           | Repository name                                              |