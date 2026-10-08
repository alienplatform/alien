# ConfigureProjectSourceGithubSetup

## Example Usage

```typescript
import { ConfigureProjectSourceGithubSetup } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceGithubSetup = {
  pullRequestUrl: "https://evil-tackle.com",
  workflowUrl: "https://total-swath.biz",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `pullRequestUrl`                                      | *string*                                              | :heavy_check_mark:                                    | URL to the pull request with the Alien build workflow |
| `workflowUrl`                                         | *string*                                              | :heavy_check_mark:                                    | URL to the GitHub Actions workflow                    |