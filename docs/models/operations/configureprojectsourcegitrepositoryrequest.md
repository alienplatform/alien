# ConfigureProjectSourceGitRepositoryRequest

## Example Usage

```typescript
import { ConfigureProjectSourceGitRepositoryRequest } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceGitRepositoryRequest = {
  type: "github",
  repo: "alien/my-agent",
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  | Example                                                                                                      |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `type`                                                                                                       | [operations.ConfigureProjectSourceTypeRequest](../../models/operations/configureprojectsourcetyperequest.md) | :heavy_check_mark:                                                                                           | The Git Provider of the repository                                                                           | github                                                                                                       |
| `repo`                                                                                                       | *string*                                                                                                     | :heavy_check_mark:                                                                                           | The name of the git repository                                                                               | alien/my-agent                                                                                               |