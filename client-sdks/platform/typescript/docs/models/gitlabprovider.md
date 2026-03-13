# GitLabProvider

## Example Usage

```typescript
import { GitLabProvider } from "@aliendotdev/platform-api/models";

let value: GitLabProvider = {
  type: "gitlab",
  namespace: "<value>",
  project: "<value>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `type`                                                       | [models.GitLabProviderType](../models/gitlabprovidertype.md) | :heavy_check_mark:                                           | N/A                                                          |
| `namespace`                                                  | *string*                                                     | :heavy_check_mark:                                           | Group/project namespace                                      |
| `project`                                                    | *string*                                                     | :heavy_check_mark:                                           | Project name                                                 |