# CreateProjectGitRepositoryResponse

Verified source repository connected to the project. Alien uses this for GitHub Actions setup and source-aware features; releases are still created explicitly by CI or `alien release`.

## Example Usage

```typescript
import { CreateProjectGitRepositoryResponse } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectGitRepositoryResponse = {
  type: "github",
  repo: "alien/my-agent",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  | Example                                                                                      |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `type`                                                                                       | [operations.CreateProjectTypeResponse](../../models/operations/createprojecttyperesponse.md) | :heavy_check_mark:                                                                           | The Git Provider of the repository                                                           | github                                                                                       |
| `repo`                                                                                       | *string*                                                                                     | :heavy_check_mark:                                                                           | The name of the git repository                                                               | alien/my-agent                                                                               |