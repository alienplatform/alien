# CreateProjectFromTemplateGithubSetup

## Example Usage

```typescript
import { CreateProjectFromTemplateGithubSetup } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectFromTemplateGithubSetup = {
  pullRequestUrl: "https://colorless-volleyball.org/",
  workflowUrl: "https://pointless-puppet.name",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `pullRequestUrl`                                      | *string*                                              | :heavy_check_mark:                                    | URL to the pull request with the Alien build workflow |
| `workflowUrl`                                         | *string*                                              | :heavy_check_mark:                                    | URL to the GitHub Actions workflow                    |