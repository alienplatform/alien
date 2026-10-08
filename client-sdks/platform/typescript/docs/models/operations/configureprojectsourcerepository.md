# ConfigureProjectSourceRepository

## Example Usage

```typescript
import { ConfigureProjectSourceRepository } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceRepository = {
  mode: "repository",
  gitRepository: {
    type: "github",
    repo: "alien/my-agent",
  },
};
```

## Fields

| Field                                                                                                                              | Type                                                                                                                               | Required                                                                                                                           | Description                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `mode`                                                                                                                             | *"repository"*                                                                                                                     | :heavy_check_mark:                                                                                                                 | N/A                                                                                                                                |
| `gitRepository`                                                                                                                    | [operations.ConfigureProjectSourceGitRepositoryRequest](../../models/operations/configureprojectsourcegitrepositoryrequest.md)     | :heavy_check_mark:                                                                                                                 | N/A                                                                                                                                |
| `rootDirectory`                                                                                                                    | *string*                                                                                                                           | :heavy_minus_sign:                                                                                                                 | The name of a directory or relative path to the source code of your project. When null is used it will default to the project root |