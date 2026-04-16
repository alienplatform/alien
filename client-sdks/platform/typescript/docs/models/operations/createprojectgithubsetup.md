# CreateProjectGithubSetup

## Example Usage

```typescript
import { CreateProjectGithubSetup } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectGithubSetup = {
  pullRequestUrl: "https://digital-icebreaker.info/",
  workflowUrl: "https://bitter-footrest.biz/",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `pullRequestUrl`                                      | *string*                                              | :heavy_check_mark:                                    | URL to the pull request with the Alien build workflow |
| `workflowUrl`                                         | *string*                                              | :heavy_check_mark:                                    | URL to the GitHub Actions workflow                    |