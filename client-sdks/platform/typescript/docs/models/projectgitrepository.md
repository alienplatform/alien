# ProjectGitRepository

The Git Repository that will be connected to the project. When this is defined, any pushes to the specified connected Git Repository will be automatically deployed

## Example Usage

```typescript
import { ProjectGitRepository } from "@aliendotdev/platform-api/models";

let value: ProjectGitRepository = {
  type: "github",
  repo: "alien/my-agent",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                | Example                                                    |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `type`                                                     | [models.ProjectTypeGithub](../models/projecttypegithub.md) | :heavy_check_mark:                                         | The Git Provider of the repository                         | github                                                     |
| `repo`                                                     | *string*                                                   | :heavy_check_mark:                                         | The name of the git repository                             | alien/my-agent                                             |